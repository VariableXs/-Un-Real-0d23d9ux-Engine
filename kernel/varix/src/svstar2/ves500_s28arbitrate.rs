//! VE-F4266 · 跨域仲裁委员会（U04 一致性治理与演进组）
//! 职责：跨域一致性争议仲裁（域间规则冲突/违例定级争议/演进僵局三类触发）
//! + 裁决终局入案例库供检索（同类争议先查库再开庭）+ 轮值主席代行
//! + 超时默认维持现状 + 裁决与红线冲突红线优先。
//! 段位：0x1FE0（续表 idx500）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1FE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArCode {
    /// 席位空缺且无主席代行。
    SeatVoid = SEG_BASE,
    /// 裁决与红线冲突（红线优先并立案）。
    RedlineConflict = SEG_BASE + 0x01,
    /// 案例库检索失配。
    CaseLost = SEG_BASE + 0x02,
    /// 排期冲突。
    ScheduleClash = SEG_BASE + 0x03,
}

/// 仲裁触发三类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArTrigger {
    RuleClash = 0,
    LevelDispute = 1,
    EvolutionDeadlock = 2,
}

/// 裁决。
#[derive(Clone, Copy, Debug)]
pub struct Ruling {
    pub id: u32,
    pub trigger: ArTrigger,
    pub topic_fp: u64, // 争议主题指纹（案例检索键）
    pub verdict: u8,   // 0 维持现状 1 甲方 2 乙方
    pub chair: u8,     // 裁决人（轮值主席代行亦记）
    pub by_proxy: bool, // 主席代行留痕
}

/// 仲裁委员会。
pub struct Arbitration {
    /// 代表席位：(域, 代表)。
    pub seats: Vec<(u8, u8)>,
    pub chair: Option<u8>,
    /// 案例库。
    pub cases: Vec<Ruling>,
    /// 排期表：(日, 主题指纹)。
    pub schedule: Vec<(u16, u64)>,
    /// 超时维持现状记录。
    pub defaults: u32,
    pub today: u16,
}

impl Arbitration {
    pub fn new(today: u16) -> Self {
        Arbitration { seats: Vec::new(), chair: None, cases: Vec::new(), schedule: Vec::new(), defaults: 0, today }
    }

    /// 席位登记。
    pub fn seat(&mut self, dom: u8, rep: u8) {
        if !self.seats.iter().any(|(d, _)| *d == dom) {
            self.seats.push((dom, rep));
        }
    }

    /// 开庭资格：至少一位代表或主席可代行。
    fn quorum(&self) -> bool {
        !self.seats.is_empty() || self.chair.is_some()
    }

    /// 排期：同日同主题冲突检。
    pub fn schedule_hearing(&mut self, day: u16, topic_fp: u64) -> Result<(), ArCode> {
        if !self.quorum() {
            return Err(ArCode::SeatVoid);
        }
        if self.schedule.iter().any(|(d, t)| *d == day && *t == topic_fp) {
            return Err(ArCode::ScheduleClash);
        }
        self.schedule.push((day, topic_fp));
        Ok(())
    }

    /// 案例检索：同类争议先查库——返回既往终局裁决。
    pub fn lookup(&self, topic_fp: u64) -> Option<&Ruling> {
        self.cases.iter().find(|c| c.topic_fp == topic_fp)
    }

    /// 开庭裁决：终局入案例库；与红线冲突→红线优先立案拒绝登记。
    pub fn rule(&mut self, id: u32, trigger: ArTrigger, topic_fp: u64, verdict: u8, redline: bool) -> Result<(), ArCode> {
        if !self.quorum() {
            return Err(ArCode::SeatVoid);
        }
        if redline {
            return Err(ArCode::RedlineConflict); // 红线优先并立案（上层处置）
        }
        if self.lookup(topic_fp).is_some() {
            return Ok(()); // 已有终局案例：不再开庭，直接援引
        }
        let chair = self.chair.unwrap_or(0);
        let by_proxy = self.seats.is_empty();
        self.cases.push(Ruling { id, trigger, topic_fp, verdict, chair, by_proxy });
        Ok(())
    }

    /// 超时未裁：默认维持现状并记录。
    pub fn timeout_default(&mut self) {
        self.defaults += 1;
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C500-01..12。
pub fn run_ves500_checks() -> CheckSet {
    let mut set = CheckSet::new("ves500");

    let mut ar = Arbitration::new(1);
    chk(&mut set, "C500-01", ar.schedule_hearing(10, 0xAA) == Err(ArCode::SeatVoid), "席位空缺无主席→拒开庭");

    ar.chair = Some(7);
    chk(&mut set, "C500-02", ar.schedule_hearing(10, 0xAA).is_ok(), "主席代行可排期");
    chk(&mut set, "C500-03", ar.schedule_hearing(10, 0xAA) == Err(ArCode::ScheduleClash), "同日同主题排期冲突");
    chk(&mut set, "C500-04", ar.schedule_hearing(10, 0xBB).is_ok(), "同日异主题放行");

    // 红线优先
    chk(&mut set, "C500-05", ar.rule(1, ArTrigger::RuleClash, 0xCC, 1, true) == Err(ArCode::RedlineConflict), "裁决与红线冲突→红线优先拒登记");
    chk(&mut set, "C500-06", ar.cases.is_empty(), "红线裁决不入案例库");

    // 终局入库
    let _ = ar.rule(2, ArTrigger::LevelDispute, 0xAA, 2, false);
    chk(&mut set, "C500-07", ar.lookup(0xAA).map(|c| c.verdict) == Some(2), "终局裁决入案例库");
    chk(&mut set, "C500-08", ar.lookup(0xAA).map(|c| c.by_proxy) == Some(true), "主席代行留痕");
    let _ = ar.rule(3, ArTrigger::EvolutionDeadlock, 0xAA, 1, false);
    chk(&mut set, "C500-09", ar.cases.len() == 1, "同类争议先查库不再开庭（终局援引）");

    // 席位齐备后代行消失
    ar.seat(1, 11);
    ar.seat(2, 22);
    let _ = ar.rule(4, ArTrigger::RuleClash, 0xDD, 1, false);
    chk(&mut set, "C500-10", ar.lookup(0xDD).map(|c| c.by_proxy) == Some(false), "席位齐备裁决非代行");

    // 超时维持现状
    ar.timeout_default();
    ar.timeout_default();
    chk(&mut set, "C500-11", ar.defaults == 2, "超时默认维持现状记录");
    chk(&mut set, "C500-12", ar.seats.len() == 2 && ar.schedule.len() == 2, "席位与排期账自洽");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arbitration_rules() {
        let mut ar = Arbitration::new(1);
        assert!(ar.schedule_hearing(1, 1).is_err());
        ar.chair = Some(1);
        assert!(ar.schedule_hearing(1, 1).is_ok());
        assert!(ar.rule(1, ArTrigger::RuleClash, 1, 1, false).is_ok());
        assert!(ar.rule(2, ArTrigger::RuleClash, 1, 0, false).is_ok());
        assert_eq!(ar.cases.len(), 1);
    }
}
