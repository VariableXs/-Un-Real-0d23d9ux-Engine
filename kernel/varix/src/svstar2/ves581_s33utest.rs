//! VE-F4347 · 一致性用户测试（U08 测试与资产组）
//! 职责：一致性影响下的用户测试——测试设计（一致性变更前后对照任务集 +
//! 挫败信号采集）+ 指标三件（任务完成率 / 违例遭遇率 / 主观负担量表）+
//! 伦理三线（知情同意 + 数据脱敏 + 可随时退出——违规即刻中止红线）+
//! 样本偏差分层抽样 + 采集失效日志兜底 + 无障碍用户组（域本色）。
//! 段位：0x40E0（续表 idx581）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x40E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UtCode {
    /// 伦理违规（即刻中止红线）。
    EthicsViolation = SEG_BASE,
    /// 样本偏差（分层抽样兜底）。
    SampleBias = SEG_BASE + 0x01,
    /// 采集失效（日志兜底）。
    CollectFail = SEG_BASE + 0x02,
    /// 任务号越界。
    TaskRange = SEG_BASE + 0x03,
}

/// 一名受试者档案。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Subject {
    pub id: u32,
    /// 分层组别（0 无障碍组——域本色 / 1..=4 常规层）。
    pub stratum: u8,
    pub consent: bool,
    pub anonymized: bool,
    /// 可随时退出标记（行使即中止其全部采集）。
    pub withdrew: bool,
}

/// 对照任务结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TaskRun {
    pub task: u8,
    pub subject: u32,
    /// 变更前完成 / 变更后完成（前后对照）。
    pub done_before: bool,
    pub done_after: bool,
    /// 违例遭遇次数。
    pub violations_hit: u32,
    /// 主观负担（1-7 量表）。
    pub burden: u8,
}

/// 用户测试台。
#[derive(Default)]
pub struct UserTest {
    pub subjects: Vec<Subject>,
    pub runs: Vec<TaskRun>,
    /// 挫败信号采集计数（rage/dead click 归集）。
    pub rage_events: u32,
    /// 日志兜底事件数（采集失效时）。
    pub log_fallback: u32,
    pub aborted: u32,
}

/// 伦理三线核验（单受试者）：同意 + 脱敏 + 未行使退出。
pub fn ethics_ok(s: &Subject) -> bool {
    s.consent && s.anonymized && !s.withdrew
}

impl UserTest {
    pub fn new() -> Self {
        UserTest { ..Default::default() }
    }

    /// 受试者入组：伦理三线不全即拒（红线前置）。
    pub fn enroll(&mut self, s: Subject) -> Result<(), UtCode> {
        if !ethics_ok(&s) {
            self.aborted += 1;
            return Err(UtCode::EthicsViolation);
        }
        if !self.subjects.iter().any(|x| x.id == s.id) {
            self.subjects.push(s);
        }
        Ok(())
    }

    /// 受试者中途行使退出：即刻中止其采集（红线）。
    pub fn withdraw(&mut self, id: u32) -> Result<(), UtCode> {
        let s = self.subjects.iter_mut().find(|s| s.id == id).ok_or(UtCode::TaskRange)?;
        s.withdrew = true;
        self.aborted += 1;
        Ok(())
    }

    /// 分层抽样核验：无障碍组样本占比不得为零且各层齐全（偏差检出）。
    pub fn strata_balanced(&self) -> bool {
        self.subjects.iter().any(|s| s.stratum == 0)
            && (1..=4u8).all(|k| self.subjects.iter().any(|s| s.stratum == k))
    }

    /// 对照任务登记（伦理在案者才可采——违规即刻中止红线）。
    pub fn record(&mut self, r: TaskRun) -> Result<(), UtCode> {
        if r.task == 0 {
            return Err(UtCode::TaskRange);
        }
        let s = self.subjects.iter().find(|s| s.id == r.subject).ok_or(UtCode::TaskRange)?;
        if !ethics_ok(s) {
            self.aborted += 1;
            return Err(UtCode::EthicsViolation);
        }
        self.runs.push(r);
        Ok(())
    }

    /// 指标三件核算。
    pub fn metrics(&self) -> (u32, u32, u32) {
        let n = self.runs.len() as u64;
        if n == 0 {
            return (0, 0, 0);
        }
        let complete_bp = (self.runs.iter().filter(|r| r.done_after).count() as u64 * 10000 / n) as u32;
        let viol_rate_bp = (self.runs.iter().map(|r| r.violations_hit as u64).sum::<u64>() * 10000 / (n * 10)) as u32;
        let burden_avg = (self.runs.iter().map(|r| r.burden as u64).sum::<u64>() / n) as u32;
        (complete_bp, viol_rate_bp, burden_avg)
    }

    /// 挫败信号采集（主通道失效 → 日志兜底计数）。
    pub fn rage(&mut self, primary_ok: bool) {
        if primary_ok {
            self.rage_events += 1;
        } else {
            self.log_fallback += 1;
        }
    }

    /// 前后对照改善断言：变更后完成率不低于变更前。
    pub fn improved(&self) -> Option<bool> {
        let n = self.runs.len();
        if n == 0 {
            return None;
        }
        let before = self.runs.iter().filter(|r| r.done_before).count();
        let after = self.runs.iter().filter(|r| r.done_after).count();
        Some(after >= before)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves581_checks() -> CheckSet {
    let mut set = CheckSet::new("ves581");
    let mut ut = UserTest::new();

    // C581-01 伦理三线入组（同意+脱敏+可退出）。
    chk(&mut set, "C581-01", ut.enroll(Subject { id: 1, stratum: 0, consent: true, anonymized: true, withdrew: false }).is_ok()
        && ut.enroll(Subject { id: 2, stratum: 1, consent: true, anonymized: true, withdrew: false }).is_ok(), "伦理齐入组");

    // C581-02 伦理缺线即刻拒（红线前置）。
    chk(&mut set, "C581-02", ut.enroll(Subject { id: 3, stratum: 1, consent: false, anonymized: true, withdrew: false })
        == Err(UtCode::EthicsViolation) && ut.aborted == 1, "缺同意红线拒");

    // C581-03 中途行使退出即刻中止。
    chk(&mut set, "C581-03", ut.withdraw(2).is_ok() && ut.aborted == 2, "退出即中止");

    // C581-04 退出者采集拒绝。
    chk(&mut set, "C581-04", ut.record(TaskRun { task: 1, subject: 2, done_before: true, done_after: true, violations_hit: 0, burden: 3 })
        == Err(UtCode::EthicsViolation), "退出者拒采");

    // C581-05 分层抽样核验（缺层 = 偏差）。
    chk(&mut set, "C581-05", !ut.strata_balanced(), "缺层偏差检出");

    // C581-06 补齐五层后分层过。
    for (id, st) in [(3u32, 2u8), (4, 3), (5, 4)] {
        let _ = ut.enroll(Subject { id, stratum: st, consent: true, anonymized: true, withdrew: false });
    }
    chk(&mut set, "C581-06", ut.strata_balanced() && ut.subjects.iter().any(|s| s.stratum == 0), "五层齐备");

    // C581-07 对照任务登记 + 无障碍组样本在册（域本色）。
    let _ = ut.record(TaskRun { task: 1, subject: 1, done_before: true, done_after: true, violations_hit: 1, burden: 2 });
    chk(&mut set, "C581-07", ut.runs.len() == 1, "对照任务在册");

    // C581-08 未入组者登记拒绝。
    chk(&mut set, "C581-08", ut.record(TaskRun { task: 1, subject: 99, done_before: true, done_after: true, violations_hit: 0, burden: 1 })
        == Err(UtCode::TaskRange), "未入组拒");

    // C581-09 挫败采集主通道 + 日志兜底。
    ut.rage(true);
    ut.rage(false);
    chk(&mut set, "C581-09", ut.rage_events == 1 && ut.log_fallback == 1, "主通道+日志兜底");

    // C581-10 指标三件核算（空台恒零）。
    let m0 = UserTest::new().metrics();
    chk(&mut set, "C581-10", m0 == (0, 0, 0), "空台指标恒零");

    // C581-11 前后对照改善断言。
    for i in 1..=4u32 {
        let _ = ut.record(TaskRun { task: 2, subject: i, done_before: i % 2 == 0, done_after: true, violations_hit: 0, burden: 2 });
    }
    chk(&mut set, "C581-11", ut.improved() == Some(true), "前后对照改善");

    // C581-12 任务号越界拒 + 空台改善 None。
    chk(&mut set, "C581-12", ut.record(TaskRun { task: 0, subject: 1, done_before: true, done_after: true, violations_hit: 0, burden: 1 })
        .is_err() && UserTest::new().improved().is_none(), "越界拒+空台None");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ethics_hardline() {
        let mut ut = UserTest::new();
        assert_eq!(ut.enroll(Subject { id: 1, stratum: 1, consent: true, anonymized: false, withdrew: false }), Err(UtCode::EthicsViolation));
        assert!(ut.enroll(Subject { id: 1, stratum: 1, consent: true, anonymized: true, withdrew: false }).is_ok());
        assert!(ut.withdraw(1).is_ok());
        assert_eq!(ut.record(TaskRun { task: 1, subject: 1, done_before: true, done_after: true, violations_hit: 0, burden: 1 }), Err(UtCode::EthicsViolation));
    }

    #[test]
    fn strata_balance() {
        let mut ut = UserTest::new();
        for (id, st) in [(1u32, 0u8), (2, 1), (3, 2), (4, 3), (5, 4)] {
            let _ = ut.enroll(Subject { id, stratum: st, consent: true, anonymized: true, withdrew: false });
        }
        assert!(ut.strata_balanced());
    }
}
