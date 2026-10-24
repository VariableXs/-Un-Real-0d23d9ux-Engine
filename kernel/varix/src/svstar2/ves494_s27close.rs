//! VE-F4260 · U03 组收口双签（U03 一致性验证平台）
//! 职责：四件（自查+联调回执+性能证明+基线双签记录）齐备双签——自查签与
//! 复核签分离禁同人；四件缺一拒签；基线未签硬性拒签；无障碍确认项在册。
//! 段位：0x1EE0（续表 idx494）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1EE0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 四件缺一拒签。
    ItemMissing = SEG_BASE,
    /// 双签同人作废。
    SameSigner = SEG_BASE + 0x01,
    /// 基线未双签硬拒。
    BaselineUnsigned = SEG_BASE + 0x02,
    /// 无障碍确认项缺（域色红线）。
    A11yMissing = SEG_BASE + 0x03,
}

/// 收口四件清单位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CloseList {
    pub self_check: bool,
    pub joint_receipt: bool,
    pub perf_proof: bool,
    pub baseline_sign: bool,
    /// 无障碍验证线确认项。
    pub a11y_confirmed: bool,
}

impl CloseList {
    pub fn four_ok(&self) -> bool {
        self.self_check && self.joint_receipt && self.perf_proof && self.baseline_sign
    }
}

/// 双签记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualSign {
    pub self_signer: u8,
    pub reviewer: u8,
}

impl DualSign {
    pub fn separated(&self) -> bool {
        self.self_signer != 0 && self.reviewer != 0 && self.self_signer != self.reviewer
    }
}

/// 收口裁决器。
pub struct Closer {
    pub list: CloseList,
    pub signs: Option<DualSign>,
    pub closed: bool,
    pub voided: u32,
    pub urges: u32,
}

impl Closer {
    pub fn new() -> Self {
        Closer { list: CloseList { self_check: false, joint_receipt: false, perf_proof: false, baseline_sign: false, a11y_confirmed: false }, signs: None, closed: false, voided: 0, urges: 0 }
    }

    /// 尝试收口：四件齐+双签分离+基线签+无障碍确认 → closed。
    pub fn try_close(&mut self, s: DualSign) -> Result<(), ClCode> {
        if !self.list.self_check || !self.list.joint_receipt || !self.list.perf_proof {
            return Err(ClCode::ItemMissing);
        }
        if !self.list.baseline_sign {
            return Err(ClCode::BaselineUnsigned); // 硬性拒签优先于同人判定
        }
        if !s.separated() {
            self.voided += 1;
            return Err(ClCode::SameSigner);
        }
        if !self.list.a11y_confirmed {
            return Err(ClCode::A11yMissing);
        }
        self.signs = Some(s);
        self.closed = true;
        Ok(())
    }

    /// 催办（缺件时）。
    pub fn urge(&mut self) {
        if !self.list.four_ok() {
            self.urges += 1;
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C494-01..12。
pub fn run_ves494_checks() -> CheckSet {
    let mut set = CheckSet::new("ves494");

    let mut cl = Closer::new();
    let s1 = DualSign { self_signer: 1, reviewer: 2 };
    chk(&mut set, "C494-01", cl.try_close(s1) == Err(ClCode::ItemMissing), "四件缺一拒签");
    cl.urge();
    chk(&mut set, "C494-02", cl.urges == 1, "缺件催办计数");

    cl.list.self_check = true;
    cl.list.joint_receipt = true;
    cl.list.perf_proof = true;
    chk(&mut set, "C494-03", cl.try_close(s1) == Err(ClCode::BaselineUnsigned), "基线未双签硬性拒签");
    cl.urge();
    chk(&mut set, "C494-04", cl.urges == 1, "三件齐但基线缺不重复催办条件（four_ok false 已计）");

    cl.list.baseline_sign = true;
    let s_same = DualSign { self_signer: 3, reviewer: 3 };
    chk(&mut set, "C494-05", cl.try_close(s_same) == Err(ClCode::SameSigner), "双签同人作废");
    chk(&mut set, "C494-06", cl.voided == 1 && !cl.closed, "作废记录且未收口");
    let s_zero = DualSign { self_signer: 0, reviewer: 2 };
    chk(&mut set, "C494-07", cl.try_close(s_zero) == Err(ClCode::SameSigner), "零号签视同无效");

    chk(&mut set, "C494-08", cl.try_close(s1) == Err(ClCode::A11yMissing), "无障碍确认项缺（域色红线）");
    cl.list.a11y_confirmed = true;
    chk(&mut set, "C494-09", cl.try_close(s1).is_ok(), "四件齐+双签分离+确认项→收口");
    chk(&mut set, "C494-10", cl.closed && cl.signs == Some(s1), "双签记录在册");

    // 分离校验独立成立
    chk(&mut set, "C494-11", s1.separated() && !s_same.separated() && !s_zero.separated(), "双签分离语义三例");
    chk(&mut set, "C494-12", CloseList { self_check: true, joint_receipt: true, perf_proof: true, baseline_sign: true, a11y_confirmed: true }.four_ok(), "四件齐语义复述");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closer_dual_sign() {
        let mut cl = Closer::new();
        assert_eq!(cl.try_close(DualSign { self_signer: 1, reviewer: 2 }), Err(ClCode::ItemMissing));
        cl.list = CloseList { self_check: true, joint_receipt: true, perf_proof: true, baseline_sign: true, a11y_confirmed: true };
        assert!(cl.try_close(DualSign { self_signer: 1, reviewer: 2 }).is_ok());
        assert!(cl.closed);
    }
}
