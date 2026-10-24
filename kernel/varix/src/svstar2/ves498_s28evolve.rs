//! VE-F4264 · 一致性演进机制（U04 一致性治理与演进组）
//! 职责：规则生命周期三态（草案/生效/废弃）+ 宽限期 + 演进触发三源
//! + 双轨期以新规则裁决并留痕 + 紧急加急通道事后追认留痕不豁免。
//! 段位：0x1FA0（续表 idx498）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1FA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvCode {
    /// 宽限期逾期未迁移（自动升级违例立案）。
    GraceOverdue = SEG_BASE,
    /// 双轨期裁决冲突未按新规则。
    DualConflict = SEG_BASE + 0x01,
    /// 紧急提案未追认。
    UrgentUnratified = SEG_BASE + 0x02,
    /// 规则不存在。
    NotFound = SEG_BASE + 0x03,
}

/// 生命周期三态。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LcState {
    Draft = 0,
    Active = 1,
    Retired = 2,
}

/// 演进触发源三。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    Trend = 0,
    MajorViolation = 1,
    PlatformChange = 2,
}

/// 规则生命周期条目。
#[derive(Clone, Copy, Debug)]
pub struct RuleLife {
    pub rule: u16,
    pub state: LcState,
    /// 宽限期截止日（0=无宽限）。
    pub grace_until: u16,
    /// 废弃后双轨期截止日。
    dual_until: u16,
    /// 双轨期裁决留痕。
    pub dual_rulings: u32,
}

/// 演进册。
pub struct EvolveBook {
    pub rules: Vec<RuleLife>,
    /// 紧急提案：(规则, 已追认)。
    pub urgents: Vec<(u16, bool)>,
    /// 提案池（触发源记录）。
    pub proposals: Vec<(u16, Trigger)>,
    pub today: u16,
}

impl EvolveBook {
    pub fn new(today: u16) -> Self {
        EvolveBook { rules: Vec::new(), urgents: Vec::new(), proposals: Vec::new(), today }
    }

    /// 规则登记（草案）。
    pub fn draft(&mut self, rule: u16) {
        if !self.rules.iter().any(|r| r.rule == rule) {
            self.rules.push(RuleLife { rule, state: LcState::Draft, grace_until: 0, dual_until: 0, dual_rulings: 0 });
        }
    }

    /// 生效（可带宽限期）。
    pub fn activate(&mut self, rule: u16, grace_until: u16) -> Result<(), EvCode> {
        let r = self.rules.iter_mut().find(|r| r.rule == rule).ok_or(EvCode::NotFound)?;
        r.state = LcState::Active;
        r.grace_until = grace_until;
        Ok(())
    }

    /// 废弃：进入双轨期（新规则裁决）。
    pub fn retire(&mut self, rule: u16, dual_until: u16) -> Result<(), EvCode> {
        let r = self.rules.iter_mut().find(|r| r.rule == rule).ok_or(EvCode::NotFound)?;
        r.state = LcState::Retired;
        r.dual_until = dual_until;
        Ok(())
    }

    /// 双轨裁决：废弃规则仍在双轨期内被引用→以新规则裁决并留痕。
    pub fn dual_rule(&mut self, rule: u16) -> Result<&'static str, EvCode> {
        let r = self.rules.iter_mut().find(|r| r.rule == rule).ok_or(EvCode::NotFound)?;
        if r.state != LcState::Retired {
            return Err(EvCode::DualConflict);
        }
        if self.today <= r.dual_until {
            r.dual_rulings += 1;
            return Ok("new"); // 以新规则裁决
        }
        Ok("off") // 双轨期满彻底下线
    }

    /// 宽限期扫描：逾期未迁移→违例立案。
    pub fn grace_sweep(&mut self) -> Vec<u16> {
        let today = self.today;
        let mut out = Vec::new();
        for r in self.rules.iter() {
            if r.state == LcState::Active && r.grace_until > 0 && today > r.grace_until {
                out.push(r.rule);
            }
        }
        out
    }

    /// 紧急加急：绕会提案登记（未追认）。
    pub fn urgent(&mut self, rule: u16) {
        self.urgents.push((rule, false));
    }

    /// 事后追认（留痕不豁免——追认后仍入提案池）。
    pub fn ratify(&mut self, rule: u16) -> Result<(), EvCode> {
        let u = self.urgents.iter_mut().find(|(r, ok)| *r == rule && !*ok).ok_or(EvCode::UrgentUnratified)?;
        u.1 = true;
        self.proposals.push((rule, Trigger::MajorViolation));
        Ok(())
    }

    /// 提案（正常三源触发）。
    pub fn propose(&mut self, rule: u16, t: Trigger) {
        self.proposals.push((rule, t));
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C498-01..12。
pub fn run_ves498_checks() -> CheckSet {
    let mut set = CheckSet::new("ves498");

    let mut eb = EvolveBook::new(100);
    eb.draft(10);
    eb.draft(20);
    chk(&mut set, "C498-01", eb.rules.len() == 2 && eb.rules.iter().all(|r| r.state == LcState::Draft), "两规则草案态");

    let _ = eb.activate(10, 90);
    let _ = eb.activate(20, 0);
    chk(&mut set, "C498-02", eb.rules.iter().all(|r| r.state == LcState::Active), "两规则生效");

    // 宽限期扫描
    eb.today = 95;
    chk(&mut set, "C498-03", eb.grace_sweep() == alloc::vec![10], "宽限逾期检出（规则10）");
    eb.today = 80;
    chk(&mut set, "C498-04", eb.grace_sweep().is_empty(), "限期内不立案");

    // 双轨期
    let _ = eb.retire(10, 120);
    eb.today = 110;
    chk(&mut set, "C498-05", eb.dual_rule(10) == Ok("new"), "双轨期内以新规则裁决");
    chk(&mut set, "C498-06", eb.rules.iter().find(|r| r.rule == 10).unwrap().dual_rulings == 1, "双轨裁决留痕");
    eb.today = 121;
    chk(&mut set, "C498-07", eb.dual_rule(10) == Ok("off"), "双轨期满彻底下线");
    chk(&mut set, "C498-08", eb.dual_rule(20) == Err(EvCode::DualConflict), "生效规则无双轨裁决语义");

    // 紧急加急追认
    eb.urgent(30);
    chk(&mut set, "C498-09", eb.ratify(30).is_ok(), "紧急提案事后追认");
    chk(&mut set, "C498-10", eb.proposals.len() == 1 && eb.proposals[0].1 == Trigger::MajorViolation, "追认留痕入提案池（不豁免）");
    chk(&mut set, "C498-11", eb.ratify(30) == Err(EvCode::UrgentUnratified), "重复追认拒收");

    // 三源提案
    eb.propose(40, Trigger::Trend);
    eb.propose(41, Trigger::PlatformChange);
    chk(&mut set, "C498-12", eb.proposals.len() == 3 && eb.proposals.iter().any(|(_, t)| *t == Trigger::Trend) && eb.proposals.iter().any(|(_, t)| *t == Trigger::PlatformChange), "演进触发三源在册");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evolve_lifecycle() {
        let mut eb = EvolveBook::new(1);
        eb.draft(1);
        assert!(eb.activate(1, 10).is_ok());
        assert!(eb.retire(1, 5).is_ok());
        eb.today = 3;
        assert_eq!(eb.dual_rule(1), Ok("new"));
        eb.today = 6;
        assert_eq!(eb.dual_rule(1), Ok("off"));
    }
}
