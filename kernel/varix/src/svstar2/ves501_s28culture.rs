//! VE-F4267 · 一致性文化建设（U04 治理组）
//! 职责：荣誉与问责双轨（一致性之星评选+违例黑榜）+ 黑榜公示期申诉撤销
//! + 知识沉淀三件（案例月报/复盘会/新人引路）+ 月报缺稿轮值兜底不停刊。
//! 段位：0x2700（续表 idx501）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2700;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CuCode {
    /// 黑榜误报（公示期申诉撤销）。
    BlacklistAppeal = SEG_BASE,
    /// 月报缺稿（轮值兜底）。
    MonthlyVacant = SEG_BASE + 0x01,
    /// 活动停办（降级线上简报）。
    EventDown = SEG_BASE + 0x02,
}

/// 荣誉台账条目。
#[derive(Clone, Copy, Debug)]
pub struct Honor {
    pub who: u8,
    pub stars: u16,
    pub month: u8,
}

/// 黑榜记录（带申诉状态）。
#[derive(Clone, Copy, Debug)]
pub struct BlackRow {
    pub who: u8,
    pub count: u16,
    pub appealed: bool,
    pub revoked: bool,
    /// 公示期剩余日。
    pub publish_left: u8,
}

/// 文化册。
pub struct CultureBook {
    pub honors: Vec<Honor>,
    pub blacklist: Vec<BlackRow>,
    /// 月报归档：(月, 稿件数, 轮值兜底)。
    pub monthly: Vec<(u8, u32, bool)>,
    /// 季度一致性日开关。
    pub quarter_day: bool,
}

impl CultureBook {
    pub fn new() -> Self {
        CultureBook { honors: Vec::new(), blacklist: Vec::new(), monthly: Vec::new(), quarter_day: false }
    }

    /// 星评选：月度入账。
    pub fn award(&mut self, who: u8, stars: u16, month: u8) {
        self.honors.push(Honor { who, stars, month });
    }

    /// 黑榜登记（公示期 7 日）。
    pub fn blacklist_add(&mut self, who: u8, count: u16) {
        if !self.blacklist.iter().any(|b| b.who == who) {
            self.blacklist.push(BlackRow { who, count, appealed: false, revoked: false, publish_left: 7 });
        }
    }

    /// 申诉：公示期内误报撤销（revoked 后不出现在有效黑榜）。
    pub fn appeal(&mut self, who: u8) -> Result<(), CuCode> {
        let b = self.blacklist.iter_mut().find(|b| b.who == who).ok_or(CuCode::BlacklistAppeal)?;
        if b.publish_left == 0 {
            return Err(CuCode::BlacklistAppeal); // 过公示期不可申诉
        }
        b.appealed = true;
        b.revoked = true;
        Ok(())
    }

    /// 有效黑榜（未撤销）。
    pub fn active_blacklist(&self) -> Vec<u8> {
        self.blacklist.iter().filter(|b| !b.revoked).map(|b| b.who).collect()
    }

    /// 月报：缺稿轮值兜底不停刊。
    pub fn monthly_report(&mut self, month: u8, articles: u32, rotator: Option<u8>) -> Result<(), CuCode> {
        if articles == 0 {
            match rotator {
                Some(_) => {
                    self.monthly.push((month, 1, true)); // 兜底一篇
                    Ok(())
                }
                None => Err(CuCode::MonthlyVacant),
            }
        } else {
            self.monthly.push((month, articles, false));
            Ok(())
        }
    }

    /// 活动降级：停办→线上简报（不断档）。
    pub fn event_fallback(&mut self, held: bool) -> Result<(), CuCode> {
        if held {
            self.quarter_day = true;
            Ok(())
        } else {
            self.quarter_day = false;
            Err(CuCode::EventDown)
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C501-01..12。
pub fn run_ves501_checks() -> CheckSet {
    let mut set = CheckSet::new("ves501");

    let mut cb = CultureBook::new();
    cb.award(1, 3, 9);
    cb.award(2, 5, 9);
    chk(&mut set, "C501-01", cb.honors.len() == 2 && cb.honors[1].stars == 5, "荣誉台账月度入账");

    cb.blacklist_add(3, 2);
    cb.blacklist_add(4, 7);
    cb.blacklist_add(3, 9); // 重复登记拒收
    chk(&mut set, "C501-02", cb.blacklist.len() == 2, "黑榜登记去重");
    chk(&mut set, "C501-03", cb.active_blacklist() == alloc::vec![3, 4], "有效黑榜双人在册");

    // 申诉撤销
    chk(&mut set, "C501-04", cb.appeal(3).is_ok(), "公示期内申诉成立撤销");
    chk(&mut set, "C501-05", cb.active_blacklist() == alloc::vec![4], "撤销后黑榜单人");
    {
        let b = cb.blacklist.iter_mut().find(|b| b.who == 4).unwrap();
        b.publish_left = 0;
    }
    chk(&mut set, "C501-06", cb.appeal(4) == Err(CuCode::BlacklistAppeal), "过公示期不可申诉");

    // 月报不停刊
    chk(&mut set, "C501-07", cb.monthly_report(9, 4, None).is_ok(), "有稿正常归档");
    chk(&mut set, "C501-08", cb.monthly_report(10, 0, Some(1)).is_ok(), "缺稿轮值兜底不停刊");
    chk(&mut set, "C501-09", cb.monthly.last().map(|m| m.2) == Some(true), "兜底标记在册");
    chk(&mut set, "C501-10", cb.monthly_report(11, 0, None) == Err(CuCode::MonthlyVacant), "无稿无兜底=断刊报错");

    // 活动降级
    chk(&mut set, "C501-11", cb.event_fallback(false) == Err(CuCode::EventDown) && !cb.quarter_day, "停办降级线上简报不断档");
    chk(&mut set, "C501-12", cb.event_fallback(true).is_ok() && cb.quarter_day, "复办开关恢复");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn culture_dual_track() {
        let mut cb = CultureBook::new();
        cb.blacklist_add(1, 1);
        assert!(cb.appeal(1).is_ok());
        assert!(cb.active_blacklist().is_empty());
        assert!(cb.monthly_report(1, 0, Some(2)).is_ok());
        assert!(cb.monthly_report(2, 0, None).is_err());
    }
}
