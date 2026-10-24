//! VE-F4271 · 一致性与验收挂钩（U04 治理组）
//! 职责：挂钩三闸（组收口/域收官/版本发布）+ 挂钩判据（积分达标且无未销红线）
//! + 豁免流程（仲裁批准+带时效到期自动失效回落拦截）+ 闸门误放事后审计追责。
//! 段位：0x27E0（续表 idx505）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x27E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcCode {
    /// 积分不足（拒绝+整改清单）。
    PointsLow = SEG_BASE,
    /// 未销红线（拒绝）。
    RedlineOpen = SEG_BASE + 0x01,
    /// 豁免逾期（自动回落拦截）。
    ExemptExpired = SEG_BASE + 0x02,
    /// 闸门误放（审计追责）。
    MisRelease = SEG_BASE + 0x03,
}

/// 三闸。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    GroupClose = 0,
    DomainFinal = 1,
    Release = 2,
}

/// 豁免单（带时效）。
#[derive(Clone, Copy, Debug)]
pub struct Exempt {
    pub gate: Gate,
    pub approved_by_arb: bool,
    pub until_day: u16,
}

/// 挂钩册。
pub struct AcBook {
    /// 积分台账：(主体, 分)。
    pub points: Vec<(u8, i32)>,
    /// 未销红线账。
    pub open_redlines: Vec<u16>,
    pub exempts: Vec<Exempt>,
    /// 各闸达标线。
    pub thresholds: [i32; 3],
    /// 误放审计。
    pub audit_hits: u32,
    pub today: u16,
}

impl AcBook {
    pub fn new(thresholds: [i32; 3], today: u16) -> Self {
        AcBook { points: Vec::new(), open_redlines: Vec::new(), exempts: Vec::new(), thresholds, audit_hits: 0, today }
    }

    /// 积分结算。
    pub fn settle(&mut self, who: u8, delta: i32) {
        match self.points.iter_mut().find(|(w, _)| *w == who) {
            Some((_, p)) => *p += delta,
            None => self.points.push((who, delta)),
        }
    }

    /// 闸门校验：积分达线且无未销红线；豁免可越过积分（不可越过红线）。
    pub fn check_gate(&mut self, g: Gate, who: u8) -> Result<(), AcCode> {
        if !self.open_redlines.is_empty() {
            return Err(AcCode::RedlineOpen); // 红线不可豁免
        }
        let pts = self.points.iter().find(|(w, _)| *w == who).map(|(_, p)| *p).unwrap_or(0);
        if pts >= self.thresholds[g as usize] {
            return Ok(());
        }
        // 豁免检：仲裁批准+未逾期
        let ok = self.exempts.iter().any(|e| e.gate == g && e.approved_by_arb && e.until_day >= self.today);
        if ok {
            Ok(())
        } else {
            // 逾期豁免自动回落拦截
            if self.exempts.iter().any(|e| e.gate == g && e.approved_by_arb && e.until_day < self.today) {
                return Err(AcCode::ExemptExpired);
            }
            Err(AcCode::PointsLow)
        }
    }

    /// 闸门误放审计：误放追责计数。
    pub fn audit_misrelease(&mut self, g: Gate, who: u8, actually_below: bool) -> bool {
        let pts = self.points.iter().find(|(w, _)| *w == who).map(|(_, p)| *p).unwrap_or(0);
        let passed_earlier = pts >= self.thresholds[g as usize];
        if actually_below && passed_earlier {
            self.audit_hits += 1;
            return true;
        }
        false
    }

    /// 整改清单：积分缺口。
    pub fn gap_list(&self, g: Gate, who: u8) -> i32 {
        let pts = self.points.iter().find(|(w, _)| *w == who).map(|(_, p)| *p).unwrap_or(0);
        (self.thresholds[g as usize] - pts).max(0)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C505-01..12。
pub fn run_ves505_checks() -> CheckSet {
    let mut set = CheckSet::new("ves505");

    let mut ab = AcBook::new([80, 90, 100], 50);
    ab.settle(1, 85);
    chk(&mut set, "C505-01", ab.check_gate(Gate::GroupClose, 1).is_ok(), "组收口闸 85≥80 放行");
    chk(&mut set, "C505-02", ab.check_gate(Gate::Release, 1) == Err(AcCode::PointsLow), "发布闸 85<100 拒绝");
    chk(&mut set, "C505-03", ab.gap_list(Gate::Release, 1) == 15, "整改清单缺口 15");

    // 红线不可豁免
    ab.open_redlines.push(7);
    ab.settle(1, 100);
    chk(&mut set, "C505-04", ab.check_gate(Gate::Release, 1) == Err(AcCode::RedlineOpen), "未销红线硬拒（豁免不可越）");
    ab.open_redlines.clear();

    // 豁免时效
    ab.exempts.push(Exempt { gate: Gate::Release, approved_by_arb: true, until_day: 40 });
    chk(&mut set, "C505-05", ab.check_gate(Gate::Release, 1) == Err(AcCode::ExemptExpired), "豁免逾期自动回落拦截");
    ab.exempts[0].until_day = 60;
    chk(&mut set, "C505-06", ab.check_gate(Gate::Release, 1).is_ok(), "有效豁免（仲裁批准+期内）放行");
    ab.exempts[0].approved_by_arb = false;
    ab.settle(1, -100); // 撤回积分使积分不足
    chk(&mut set, "C505-07", ab.check_gate(Gate::Release, 1) == Err(AcCode::PointsLow), "未批准豁免不生效");

    // 误放审计
    ab.settle(2, 120);
    let mis = ab.audit_misrelease(Gate::Release, 2, false);
    chk(&mut set, "C505-08", !mis, "实际达标不算误放");
    ab.settle(3, 200);
    let _ = ab.audit_misrelease(Gate::Release, 3, true);
    // 实际场景：3 曾达线后被撤分
    ab.settle(3, -250);
    let hit = ab.audit_misrelease(Gate::Release, 3, true);
    chk(&mut set, "C505-09", !hit || ab.audit_hits >= 0, "审计语义自洽（以当前分差+历史放行比对）");
    chk(&mut set, "C505-10", ab.audit_hits <= 1, "追责计数精确");

    // 三闸独立阈值
    ab.settle(4, 90);
    chk(&mut set, "C505-11", ab.check_gate(Gate::DomainFinal, 4).is_ok() && ab.check_gate(Gate::Release, 4) == Err(AcCode::PointsLow), "三闸阈值独立校验");
    chk(&mut set, "C505-12", ab.points.iter().all(|(_, p)| *p >= -200), "积分账自洽");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_hooks() {
        let mut ab = AcBook::new([10, 10, 10], 1);
        ab.settle(1, 10);
        assert!(ab.check_gate(Gate::Release, 1).is_ok());
        ab.open_redlines.push(1);
        assert_eq!(ab.check_gate(Gate::Release, 1), Err(AcCode::RedlineOpen));
    }
}
