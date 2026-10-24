//! VE-F4314 · 场景 ROI 量化（U06 场景应用组）
//! 职责：场景应用投资回报量化——ROI 模型（拦截成本 vs 修复成本 + 返工
//! 避免量）+ 度量窗口季度滚动 + ROI 报告驱动场景取舍（低 ROI 场景降级为
//! 提示型触达——不删除只降级）+ 成本口径漂移版本戳校验 + 数据缺口标注
//! 不补造（诚实呈现）+ 报告附数据表（域本色）。
//! 段位：0x31E0（续表 idx548）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x31E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoiErr {
    /// 成本口径漂移（版本戳不符拒绝结算）。
    CaliberDrift = SEG_BASE,
    /// 数据缺口（标注不补造——拒绝臆测值）。
    DataGap = SEG_BASE + 0x01,
    /// 决策争议（转仲裁复核）。
    DecisionDispute = SEG_BASE + 0x02,
    /// 窗口未满（不滚动不结算）。
    WindowShort = SEG_BASE + 0x03,
}

/// 场景 ROI 账目（成本口径版本戳绑定）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoiEntry {
    pub scene: u8,
    /// 拦截成本（工时 bp 计——越早拦越便宜）。
    pub intercept_cost: u32,
    /// 若未拦截的修复成本。
    pub fix_cost: u32,
    /// 返工避免量（次）。
    pub rework_avoided: u32,
    /// 口径版本戳。
    pub caliber: u32,
}

/// 场景触达档位（低 ROI 只降级不删除）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TouchLevel {
    Full,
    Hint,
}

/// ROI 台。
#[derive(Default)]
pub struct RoiDesk {
    pub entries: Vec<RoiEntry>,
    /// 当前口径单源版本戳。
    pub caliber_now: u32,
    /// 触达档位表。
    pub touches: Vec<(u8, TouchLevel)>,
    /// 数据缺口标注表（不补造）。
    pub gaps: Vec<u8>,
    /// 季度窗口长（天）。
    window_days: u32,
    elapsed: u32,
    pub roll_count: u32,
    /// 争议决策单。
    pub disputes: Vec<u8>,
}

/// 净收益：修复成本 − 拦截成本 × 返工避免权重（正 = 划算）。
pub fn net_benefit(e: &RoiEntry) -> i64 {
    let weight = 1 + (e.rework_avoided.min(4) as i64);
    e.fix_cost as i64 * weight - e.intercept_cost as i64
}

impl RoiDesk {
    pub fn new(caliber: u32, window_days: u32) -> Self {
        RoiDesk {
            caliber_now: caliber,
            window_days: window_days.max(1),
            ..Default::default()
        }
    }

    /// 结算入账：口径不符拒绝（漂移即拒）；零值缺口标注不补造。
    pub fn settle(&mut self, e: RoiEntry) -> Result<i64, RoiErr> {
        if e.caliber != self.caliber_now {
            return Err(RoiErr::CaliberDrift);
        }
        if e.intercept_cost == 0 && e.fix_cost == 0 && e.rework_avoided == 0 {
            // 纯零账 = 数据缺口：标注不补造（不臆测）。
            if !self.gaps.contains(&e.scene) {
                self.gaps.push(e.scene);
            }
            return Err(RoiErr::DataGap);
        }
        let nb = net_benefit(&e);
        if let Some(x) = self.entries.iter_mut().find(|x| x.scene == e.scene) {
            *x = e;
        } else {
            self.entries.push(e);
        }
        Ok(nb)
    }

    /// 季度滚动：窗口未满不滚动。
    pub fn roll(&mut self) -> Result<(), RoiErr> {
        if self.elapsed < self.window_days {
            return Err(RoiErr::WindowShort);
        }
        self.roll_count += 1;
        self.elapsed = 0;
        Ok(())
    }

    pub fn advance(&mut self, days: u32) {
        self.elapsed += days;
    }

    /// 取舍决策：净收益为负的场景降级为提示型触达（不删除——只降级）。
    pub fn decide(&mut self) -> usize {
        let mut demoted = 0;
        for e in self.entries.iter() {
            let level = if net_benefit(e) < 0 { TouchLevel::Hint } else { TouchLevel::Full };
            let scene = e.scene;
            if let Some(t) = self.touches.iter_mut().find(|t| t.0 == scene) {
                if t.1 != level {
                    if level == TouchLevel::Hint {
                        demoted += 1;
                    }
                    t.1 = level;
                }
            } else {
                if level == TouchLevel::Hint {
                    demoted += 1;
                }
                self.touches.push((scene, level));
            }
        }
        demoted
    }

    /// 决策争议：登记转仲裁复核（复议后可翻案）。
    pub fn dispute(&mut self, scene: u8) {
        if !self.disputes.contains(&scene) {
            self.disputes.push(scene);
        }
    }

    /// 仲裁翻案：争议场景恢复 Full（留痕：disputes 移除）。
    pub fn overturn(&mut self, scene: u8) -> Result<(), RoiErr> {
        if !self.disputes.contains(&scene) {
            return Err(RoiErr::DecisionDispute);
        }
        self.disputes.retain(|s| *s != scene);
        if let Some(t) = self.touches.iter_mut().find(|t| t.0 == scene) {
            t.1 = TouchLevel::Full;
        }
        Ok(())
    }

    /// 报告附数据表（域本色）：每景一行的 (scene, 净收益) 表。
    pub fn report_table(&self) -> Vec<(u8, i64)> {
        self.entries.iter().map(|e| (e.scene, net_benefit(e))).collect()
    }

    /// 缺口诚实呈现：缺口景号在报告中显性（不补造值）。
    pub fn gap_visible(&self, scene: u8) -> bool {
        self.gaps.contains(&scene)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves548_checks() -> CheckSet {
    let mut set = CheckSet::new("ves548");
    let mut rd = RoiDesk::new(7, 90);

    // C548-01 ROI 结算：拦截划算为正。
    let nb = rd.settle(RoiEntry { scene: 1, intercept_cost: 10, fix_cost: 50, rework_avoided: 2, caliber: 7 });
    chk(&mut set, "C548-01", nb == Ok(50 * 3 - 10), "正收益结算");

    // C548-02 负收益结算（拦截成本高于修复）。
    let nb2 = rd.settle(RoiEntry { scene: 2, intercept_cost: 80, fix_cost: 20, rework_avoided: 0, caliber: 7 });
    chk(&mut set, "C548-02", nb2 == Ok(20 - 80), "负收益结算");

    // C548-03 口径漂移拒绝结算。
    chk(&mut set, "C548-03", rd.settle(RoiEntry { scene: 3, intercept_cost: 1, fix_cost: 2, rework_avoided: 0, caliber: 8 })
        == Err(RoiErr::CaliberDrift), "口径漂移拒");

    // C548-04 数据缺口标注不补造。
    chk(&mut set, "C548-04", rd.settle(RoiEntry { scene: 4, intercept_cost: 0, fix_cost: 0, rework_avoided: 0, caliber: 7 })
        == Err(RoiErr::DataGap) && rd.gap_visible(4), "缺口标注不补造");

    // C548-05 窗口未满不滚动。
    rd.advance(30);
    chk(&mut set, "C548-05", rd.roll() == Err(RoiErr::WindowShort), "窗口未满不滚");

    // C548-06 窗口满滚动（计数留痕）。
    rd.advance(60);
    chk(&mut set, "C548-06", rd.roll().is_ok() && rd.roll_count == 1, "窗口满滚动");

    // C548-07 取舍决策：负收益场景降级为提示型。
    let demoted = rd.decide();
    chk(&mut set, "C548-07", demoted == 1 && rd.touches.iter().any(|t| t.0 == 2 && t.1 == TouchLevel::Hint), "低 ROI 降级");

    // C548-08 降级不删除（场景 2 仍在触达表）。
    chk(&mut set, "C548-08", rd.touches.iter().any(|t| t.0 == 2), "降级不删除");

    // C548-09 决策争议转仲裁。
    rd.dispute(2);
    chk(&mut set, "C548-09", rd.disputes == [2], "争议入仲裁");

    // C548-10 仲裁翻案恢复 Full。
    chk(&mut set, "C548-10", rd.overturn(2).is_ok()
        && rd.touches.iter().any(|t| t.0 == 2 && t.1 == TouchLevel::Full), "翻案恢复");

    // C548-11 非争议景翻案拒绝。
    chk(&mut set, "C548-11", rd.overturn(9) == Err(RoiErr::DecisionDispute), "非争议翻案拒");

    // C548-12 报告附数据表（域本色）+ 重复争议幂等。
    rd.dispute(2);
    rd.dispute(2);
    let table = rd.report_table();
    chk(&mut set, "C548-12", table == [(1, 140), (2, -60)] && rd.disputes == [2], "数据表+争议幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caliber_gate() {
        let mut rd = RoiDesk::new(5, 30);
        assert!(rd.settle(RoiEntry { scene: 1, intercept_cost: 1, fix_cost: 2, rework_avoided: 0, caliber: 5 }).is_ok());
        assert_eq!(rd.settle(RoiEntry { scene: 1, intercept_cost: 1, fix_cost: 2, rework_avoided: 0, caliber: 6 }), Err(RoiErr::CaliberDrift));
    }

    #[test]
    fn gap_no_fabrication() {
        let mut rd = RoiDesk::new(5, 30);
        let zero = RoiEntry { scene: 3, intercept_cost: 0, fix_cost: 0, rework_avoided: 0, caliber: 5 };
        assert_eq!(rd.settle(zero), Err(RoiErr::DataGap));
        assert!(rd.gap_visible(3));
        assert!(rd.entries.iter().all(|e| e.scene != 3)); // 不入账
    }

    #[test]
    fn demote_not_delete() {
        let mut rd = RoiDesk::new(5, 30);
        let _ = rd.settle(RoiEntry { scene: 2, intercept_cost: 99, fix_cost: 1, rework_avoided: 0, caliber: 5 });
        assert_eq!(rd.decide(), 1);
        assert_eq!(rd.touches, [(2, TouchLevel::Hint)]);
        rd.dispute(2);
        assert!(rd.overturn(2).is_ok());
        assert_eq!(rd.touches, [(2, TouchLevel::Full)]);
    }
}
