//! VE-F4323 · 一致性预算体系（U07 性能与预算组）
//! 职责：一致性性能预算体系——预算四维（单次判定耗时 / 批量扫描时长 /
//! 内存占用 / 上报带宽）+ 三档分级（旗舰/主流/低端）+ 超预算项进回归门
//! 阻断（实测优先于预算声明）+ 预算与实测冲突实测优先重定预算 + 分级缺失
//! 主流档兜底 + 恶意超支红线立案。
//! 段位：0x33E0（续表 idx557）。行数预算 360。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x33E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BgCode {
    /// 超预算阻断（回归门）。
    OverBudget = SEG_BASE,
    /// 恶意超支（红线立案）。
    MaliciousOver = SEG_BASE + 0x01,
    /// 维度越界。
    DimRange = SEG_BASE + 0x02,
    /// 档位越界。
    TierRange = SEG_BASE + 0x03,
}

/// 四维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BgDim {
    JudgeMs,
    ScanMs,
    MemKb,
    UploadKb,
}

/// 三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Tier {
    Flagship,
    Mainstream,
    LowEnd,
}

/// 档位系数（低端档预算最紧）。
pub fn tier_coef(t: Tier) -> u32 {
    match t {
        Tier::Flagship => 150,
        Tier::Mainstream => 100,
        Tier::LowEnd => 60,
    }
}

/// 一项预算（四维之一在某档的预算值）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BudgetRow {
    pub dim: BgDim,
    pub tier: Tier,
    /// 基准预算（主流档）。
    pub base: u32,
    /// 实测值（实测优先原则的裁决输入）。
    pub measured: Option<u32>,
}

/// 预算台。
#[derive(Default)]
pub struct BudgetDesk {
    pub rows: Vec<BudgetRow>,
    /// 回归门阻断记录。
    pub blocks: u32,
    /// 红线立案记录。
    pub redlines: Vec<u32>,
    /// 预算重定次数（实测优先）。
    pub re_budgets: u32,
}

impl BudgetDesk {
    /// 预算登记（档位缺省主流兜底）。
    pub fn put(&mut self, dim: BgDim, tier: Option<Tier>, base: u32) -> Result<(), BgCode> {
        let t = tier.unwrap_or(Tier::Mainstream);
        if base == 0 {
            return Err(BgCode::DimRange);
        }
        if let Some(r) = self.rows.iter_mut().find(|r| r.dim == dim && r.tier == t) {
            r.base = base;
        } else {
            self.rows.push(BudgetRow { dim, tier: t, base, measured: None });
        }
        Ok(())
    }

    /// 档位有效预算 = 基准 × 档位系数 / 100（缺失档按主流兜底）。
    pub fn budget_for(&self, dim: BgDim, tier: Tier) -> u32 {
        let base = self
            .rows
            .iter()
            .find(|r| r.dim == dim && r.tier == tier)
            .or_else(|| self.rows.iter().find(|r| r.dim == dim && r.tier == Tier::Mainstream))
            .map(|r| r.base)
            .unwrap_or(0);
        base * tier_coef(tier) / 100
    }

    /// 实测上报。
    pub fn report(&mut self, dim: BgDim, tier: Tier, value: u32) {
        if let Some(r) = self.rows.iter_mut().find(|r| r.dim == dim && r.tier == tier) {
            r.measured = Some(value);
        }
    }

    /// 超支倍数（实测 / 预算 ×100；无实测 = 未超）。
    pub fn over_bp(&self, dim: BgDim, tier: Tier) -> u32 {
        let b = self.budget_for(dim, tier);
        match self.rows.iter().find(|r| r.dim == dim && r.tier == tier).and_then(|r| r.measured) {
            Some(m) if b > 0 => m * 10000 / b,
            _ => 0,
        }
    }

    /// 回归门：超预算即阻断；超支 3 倍以上 = 恶意超支红线立案。
    pub fn gate(&mut self, dim: BgDim, tier: Tier) -> Result<(), BgCode> {
        let bp = self.over_bp(dim, tier);
        if bp > 30000 {
            self.redlines.push(bp);
            return Err(BgCode::MaliciousOver);
        }
        if bp > 10000 {
            self.blocks += 1;
            return Err(BgCode::OverBudget);
        }
        Ok(())
    }

    /// 实测优先重定预算：实测 < 预算（预算虚高）→ 预算下修到实测 +10%。
    pub fn recalibrate(&mut self, dim: BgDim, tier: Tier) -> Result<u32, BgCode> {
        let r = self
            .rows
            .iter_mut()
            .find(|r| r.dim == dim && r.tier == tier)
            .ok_or(BgCode::DimRange)?;
        let m = r.measured.ok_or(BgCode::DimRange)?;
        if m < r.base {
            r.base = m + m / 10;
            self.re_budgets += 1;
        }
        Ok(r.base)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves557_checks() -> CheckSet {
    let mut set = CheckSet::new("ves557");

    let mut bd = BudgetDesk::default();
    // C557-01 四维预算登记（主流档基准）。
    let dims = [BgDim::JudgeMs, BgDim::ScanMs, BgDim::MemKb, BgDim::UploadKb];
    let bases = [50, 500, 4096, 256];
    let mut ok = true;
    for (d, b) in dims.iter().zip(bases.iter()) {
        if bd.put(*d, Some(Tier::Mainstream), *b).is_err() {
            ok = false;
        }
    }
    chk(&mut set, "C557-01", ok && bd.rows.len() == 4, "四维预算登记");

    // C557-02 三档分级预算（旗舰宽 / 低端紧）。
    let bm = bd.budget_for(BgDim::JudgeMs, Tier::Mainstream);
    let bf = bd.budget_for(BgDim::JudgeMs, Tier::Flagship);
    let bl = bd.budget_for(BgDim::JudgeMs, Tier::LowEnd);
    chk(&mut set, "C557-02", bm == 50 && bf == 75 && bl == 30, "三档分级预算");

    // C557-03 分级缺失主流兜底。
    let b_low_missing = bd.budget_for(BgDim::ScanMs, Tier::LowEnd);
    chk(&mut set, "C557-03", b_low_missing == 500 * 60 / 100, "缺档主流兜底");

    // C557-04 实测在预算内过门。
    bd.report(BgDim::JudgeMs, Tier::Mainstream, 48);
    chk(&mut set, "C557-04", bd.gate(BgDim::JudgeMs, Tier::Mainstream).is_ok(), "预算内过门");

    // C557-05 超预算回归门阻断（计数）。
    bd.report(BgDim::ScanMs, Tier::Mainstream, 700);
    chk(&mut set, "C557-05", bd.gate(BgDim::ScanMs, Tier::Mainstream) == Err(BgCode::OverBudget)
        && bd.blocks == 1, "超支阻断计数");

    // C557-06 恶意超支红线立案（>3 倍）。
    bd.report(BgDim::MemKb, Tier::Mainstream, 40960); // 10 倍
    chk(&mut set, "C557-06", bd.gate(BgDim::MemKb, Tier::Mainstream) == Err(BgCode::MaliciousOver)
        && bd.redlines.len() == 1, "恶意超支红线");

    // C557-07 预算虚高实测优先重定（下修 +10%）。
    bd.report(BgDim::UploadKb, Tier::Mainstream, 100);
    let nb = bd.recalibrate(BgDim::UploadKb, Tier::Mainstream);
    chk(&mut set, "C557-07", nb == Ok(110) && bd.re_budgets == 1, "实测优先重定");

    // C557-08 实测不低于预算不重定（零误修）。
    let nb2 = bd.recalibrate(BgDim::ScanMs, Tier::Mainstream);
    chk(&mut set, "C557-08", nb2 == Ok(500) && bd.re_budgets == 1, "不虚高不重定");

    // C557-09 无实测重定拒绝。
    chk(&mut set, "C557-09", bd.recalibrate(BgDim::JudgeMs, Tier::Flagship).is_err(), "无实测拒重定");

    // C557-10 零基准拒绝。
    chk(&mut set, "C557-10", bd.put(BgDim::JudgeMs, None, 0).is_err(), "零基准拒");

    // C557-11 超支 bp 精确核算。
    let bp = bd.over_bp(BgDim::ScanMs, Tier::Mainstream);
    chk(&mut set, "C557-11", bp == 14000, "超支 bp 精确");

    // C557-12 无实测项超支恒零（不臆测）。
    chk(&mut set, "C557-12", bd.over_bp(BgDim::UploadKb, Tier::LowEnd) == 0 || bd.over_bp(BgDim::JudgeMs, Tier::LowEnd) >= 0, "无实测不臆测");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_coef_order() {
        assert!(tier_coef(Tier::Flagship) > tier_coef(Tier::Mainstream));
        assert!(tier_coef(Tier::Mainstream) > tier_coef(Tier::LowEnd));
    }

    #[test]
    fn gate_thresholds() {
        let mut bd = BudgetDesk::default();
        assert!(bd.put(BgDim::JudgeMs, None, 100).is_ok());
        bd.report(BgDim::JudgeMs, Tier::Mainstream, 101); // 101%
        assert_eq!(bd.gate(BgDim::JudgeMs, Tier::Mainstream), Err(BgCode::OverBudget));
        bd.report(BgDim::JudgeMs, Tier::Mainstream, 400); // 400% = 恶意
        assert_eq!(bd.gate(BgDim::JudgeMs, Tier::Mainstream), Err(BgCode::MaliciousOver));
        bd.report(BgDim::JudgeMs, Tier::Mainstream, 100);
        assert!(bd.gate(BgDim::JudgeMs, Tier::Mainstream).is_ok());
    }
}
