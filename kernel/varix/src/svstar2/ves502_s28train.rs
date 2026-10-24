//! VE-F4268 · 一致性培训体系（U04 治理组）
//! 职责：课程三级（新人通识/骨干实务/专家判例）+ 考核通过线与再认证双轨
//! + 题库泄漏轮换审计 + 考核未过限权推送回访 + 词典更新触发课程修订单。
//! 段位：0x2720（续表 idx502）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2720;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrCode {
    /// 考核未过（限权+回访）。
    NotPassed = SEG_BASE,
    /// 题库泄漏（轮换+审计）。
    LeakHit = SEG_BASE + 0x01,
    /// 再认证逾期。
    ReCertOverdue = SEG_BASE + 0x02,
    /// 课程过时（未随词典修订）。
    CourseStale = SEG_BASE + 0x03,
}

/// 课程级。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Tier {
    Newbie = 0,
    Core = 1,
    Expert = 2,
}

/// 学员记录。
#[derive(Clone, Copy, Debug)]
pub struct Learner {
    pub who: u8,
    pub tier: Tier,
    pub score: u8,       // 百分制
    pub pass_line: u8,
    pub limited: bool,   // 限权
    pub revisit_pushed: bool,
    pub recert_due: u16, // 再认证截止日
}

/// 培训册。
pub struct TrainBook {
    pub learners: Vec<Learner>,
    /// 题库：(代, 题数, 泄漏标记)。
    pub banks: Vec<(u8, u32, bool)>,
    /// 课程修订单：(课程, 绑定词典版本)。
    pub revisions: Vec<(u8, u32)>,
    pub today: u16,
}

impl TrainBook {
    pub fn new(today: u16) -> Self {
        TrainBook { learners: Vec::new(), banks: Vec::new(), revisions: Vec::new(), today }
    }

    /// 报名。
    pub fn enroll(&mut self, who: u8, tier: Tier, pass_line: u8, recert_due: u16) {
        if !self.learners.iter().any(|l| l.who == who) {
            self.learners.push(Learner { who, tier, score: 0, pass_line, limited: false, revisit_pushed: false, recert_due });
        }
    }

    /// 考核：未过→限权+推送回访（双轨同时）。
    pub fn examine(&mut self, who: u8, score: u8) -> Result<(), TrCode> {
        let l = self.learners.iter_mut().find(|l| l.who == who).ok_or(TrCode::NotPassed)?;
        l.score = score;
        if score < l.pass_line {
            l.limited = true;
            l.revisit_pushed = true;
            return Err(TrCode::NotPassed);
        }
        l.limited = false;
        Ok(())
    }

    /// 题库泄漏检：泄漏→轮换代+审计留痕。
    pub fn leak_sweep(&mut self, gen: u8, leaked: bool) -> Result<u8, TrCode> {
        let (cnt, leaked_flag) = {
            let b = self.banks.iter_mut().find(|b| b.0 == gen).ok_or(TrCode::LeakHit)?;
            if leaked {
                b.2 = true;
            }
            (b.1, b.2)
        };
        if leaked {
            let next = gen + 1;
            self.banks.push((next, cnt, false)); // 轮换代
            return Ok(next);
        }
        Ok(gen)
    }

    /// 再认证扫描：逾期检出。
    pub fn recert_sweep(&self) -> Vec<u8> {
        self.learners.iter().filter(|l| l.recert_due > 0 && self.today > l.recert_due).map(|l| l.who).collect()
    }

    /// 词典更新触发课程修订单（版本联动）。
    pub fn dict_updated(&mut self, course: u8, dict_ver: u32) {
        self.revisions.push((course, dict_ver));
    }

    /// 课程新鲜度：绑定词典版本 ≥ 当前词典版。
    pub fn course_fresh(&self, course: u8, dict_ver: u32) -> bool {
        self.revisions.iter().any(|(c, v)| *c == course && *v >= dict_ver)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C502-01..12。
pub fn run_ves502_checks() -> CheckSet {
    let mut set = CheckSet::new("ves502");

    let mut tb = TrainBook::new(100);
    tb.enroll(1, Tier::Newbie, 60, 200);
    tb.enroll(2, Tier::Core, 70, 90);
    tb.enroll(3, Tier::Expert, 80, 0);
    chk(&mut set, "C502-01", tb.learners.len() == 3, "三级课程报名在册");

    // 考核双轨
    chk(&mut set, "C502-02", tb.examine(1, 55) == Err(TrCode::NotPassed), "未过线报错");
    chk(&mut set, "C502-03", tb.learners.iter().find(|l| l.who == 1).map(|l| l.limited && l.revisit_pushed) == Some(true), "限权+回访双轨同时");
    chk(&mut set, "C502-04", tb.examine(1, 65).is_ok(), "回访后再考转绿");
    chk(&mut set, "C502-05", tb.learners.iter().find(|l| l.who == 1).map(|l| !l.limited) == Some(true), "限权解除");

    // 题库轮换
    tb.banks.push((1, 50, false));
    chk(&mut set, "C502-06", tb.leak_sweep(1, true) == Ok(2), "泄漏→轮换代 2");
    chk(&mut set, "C502-07", tb.banks.iter().find(|b| b.0 == 1).map(|b| b.2) == Some(true), "旧代泄漏审计留痕");
    chk(&mut set, "C502-08", tb.banks.iter().find(|b| b.0 == 2).map(|b| !b.2) == Some(true), "新代未泄漏");
    chk(&mut set, "C502-09", tb.leak_sweep(9, false) == Err(TrCode::LeakHit), "未知代报错");

    // 再认证
    let overdue = tb.recert_sweep();
    chk(&mut set, "C502-10", overdue == alloc::vec![2], "再认证逾期检出（学员2 期限 90<100）");
    chk(&mut set, "C502-11", !tb.recert_sweep().contains(&3), "零期限学员免扫");

    // 词典联动
    tb.dict_updated(1, 7);
    chk(&mut set, "C502-12", tb.course_fresh(1, 7) && !tb.course_fresh(1, 8) && !tb.course_fresh(2, 7), "课程新鲜度版本联动");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn train_tiers() {
        let mut tb = TrainBook::new(10);
        tb.enroll(1, Tier::Core, 70, 5);
        assert!(tb.examine(1, 80).is_ok());
        assert!(tb.examine(1, 50).is_err());
        assert!(tb.recert_sweep() == alloc::vec![1]);
    }
}
