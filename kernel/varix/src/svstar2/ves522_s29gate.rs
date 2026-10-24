//! VE-F4288 · 一致性×市场门禁（U05 工具集成组）
//! 职责：主题/插件上架市场一致性门禁（交互词典符合度/令牌合规/无障碍基线
//! 三项）+ 分级上架（全绿直上/黄项带标/红项拒收）+ 申诉复审改判留痕
//! + 带标项违规升级强制下架 + 检查超时暂缓不误杀 + 无障碍基线强制。
//! 段位：0x2AE0（续表 idx522）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2AE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MgCode {
    /// 红项拒收。
    RedRejected = SEG_BASE,
    /// 检查超时（暂缓不误杀）。
    TimeoutHold = SEG_BASE + 0x01,
    /// 带标违规升级（强制下架）。
    ForcedOff = SEG_BASE + 0x02,
    /// 申诉无效。
    AppealInvalid = SEG_BASE + 0x03,
}

/// 门禁三项。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MgItem {
    Dict = 0,     // 交互词典符合度
    Token = 1,    // 令牌合规
    A11yBase = 2, // 无障碍基线（强制）
}

/// 检查结论级。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum MgGrade {
    Green = 0,
    Yellow = 1,
    Red = 2,
}

/// 上架资产。
#[derive(Clone, Copy, Debug)]
pub struct Asset {
    pub id: u16,
    /// 三项结论。
    pub grades: [MgGrade; 3],
    pub listed: bool,
    pub tagged: bool, // 黄项带标上架
    pub appealed: bool,
    pub off_shelf: bool,
}

/// 门禁器。
pub struct MarketGate {
    pub assets: Vec<Asset>,
    /// 改判留痕：(资产, 旧级, 新级)。
    pub appeals: Vec<(u16, MgGrade, MgGrade)>,
    pub timeouts: u32,
}

impl MarketGate {
    pub fn new() -> Self {
        MarketGate { assets: Vec::new(), appeals: Vec::new(), timeouts: 0 }
    }

    /// 检查：三项取最严级；超时→暂缓（None）。
    pub fn inspect(&mut self, id: u16, grades: [MgGrade; 3], timeout: bool) -> Option<MgGrade> {
        if timeout {
            self.timeouts += 1;
            return None; // 暂缓不误杀
        }
        let worst = grades.iter().copied().max().unwrap_or(MgGrade::Red);
        let a = Asset { id, grades, listed: false, tagged: false, appealed: false, off_shelf: false };
        self.assets.push(a);
        Some(worst)
    }

    /// 分级上架：绿直上/黄带标/红拒收。
    pub fn list(&mut self, id: u16, grade: MgGrade) -> Result<bool, MgCode> {
        let a = self.assets.iter_mut().find(|a| a.id == id).ok_or(MgCode::RedRejected)?;
        match grade {
            MgGrade::Green => {
                a.listed = true;
                Ok(false)
            }
            MgGrade::Yellow => {
                a.listed = true;
                a.tagged = true;
                Ok(true) // 带标
            }
            MgGrade::Red => Err(MgCode::RedRejected),
        }
    }

    /// 申诉复审改判（留痕）。
    pub fn appeal(&mut self, id: u16, new_grade: MgGrade) -> Result<bool, MgCode> {
        let a = self.assets.iter_mut().find(|a| a.id == id).ok_or(MgCode::AppealInvalid)?;
        let old = a.grades.iter().copied().max().unwrap_or(MgGrade::Red);
        a.appealed = true;
        self.appeals.push((id, old, new_grade));
        match new_grade {
            MgGrade::Red => Err(MgCode::AppealInvalid),
            MgGrade::Green => {
                a.listed = true;
                a.tagged = false;
                Ok(false)
            }
            MgGrade::Yellow => {
                a.listed = true;
                a.tagged = true;
                Ok(true)
            }
        }
    }

    /// 带标项违规升级→强制下架。
    pub fn escalate(&mut self, id: u16) -> Result<(), MgCode> {
        let a = self.assets.iter_mut().find(|a| a.id == id).ok_or(MgCode::ForcedOff)?;
        if !a.tagged {
            return Err(MgCode::ForcedOff);
        }
        a.listed = false;
        a.off_shelf = true;
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C522-01..12。
pub fn run_ves522_checks() -> CheckSet {
    let mut set = CheckSet::new("ves522");

    let mut mg = MarketGate::new();
    // 三项门禁
    chk(&mut set, "C522-01", mg.inspect(1, [MgGrade::Green; 3], false) == Some(MgGrade::Green), "全绿资产");
    chk(&mut set, "C522-02", mg.inspect(2, [MgGrade::Green, MgGrade::Yellow, MgGrade::Green], false) == Some(MgGrade::Yellow), "黄项资产（取最严）");
    chk(&mut set, "C522-03", mg.inspect(3, [MgGrade::Green, MgGrade::Red, MgGrade::Green], false) == Some(MgGrade::Red), "红项资产");
    chk(&mut set, "C522-04", mg.inspect(4, [MgGrade::Green; 3], true).is_none(), "超时暂缓不误杀");
    chk(&mut set, "C522-05", mg.timeouts == 1, "超时计数");

    // 分级上架
    chk(&mut set, "C522-06", mg.list(1, MgGrade::Green) == Ok(false), "全绿直上");
    chk(&mut set, "C522-07", mg.list(2, MgGrade::Yellow) == Ok(true), "黄项带标上架");
    chk(&mut set, "C522-08", mg.list(3, MgGrade::Red) == Err(MgCode::RedRejected), "红项拒收");

    // 申诉改判
    chk(&mut set, "C522-09", mg.appeal(2, MgGrade::Green) == Ok(false), "申诉成立改判绿去标");
    chk(&mut set, "C522-10", mg.appeals.len() == 1 && mg.appeals[0].1 == MgGrade::Yellow, "改判留痕（旧级黄）");
    chk(&mut set, "C522-11", mg.appeal(2, MgGrade::Red) == Err(MgCode::AppealInvalid), "改判红无效");

    // 强制下架
    let _ = mg.list(2, MgGrade::Yellow);
    chk(&mut set, "C522-12", mg.escalate(2).is_ok() && !mg.assets.iter().any(|a| a.id == 2 && a.listed), "带标违规升级强制下架");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_gate() {
        let mut mg = MarketGate::new();
        assert_eq!(mg.inspect(1, [MgGrade::Green; 3], false), Some(MgGrade::Green));
        assert_eq!(mg.list(1, MgGrade::Green), Ok(false));
        assert_eq!(mg.inspect(2, [MgGrade::Red; 3], true), None);
        assert_eq!(mg.list(1, MgGrade::Red), Err(MgCode::RedRejected));
    }
}
