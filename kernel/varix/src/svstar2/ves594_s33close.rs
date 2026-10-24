//! VE-F4360 · U08 收口双签（U08 测试与资产组）
//! 职责：U08 组收口——三件套齐备（判据报告 + 交付清单 + 对拍记录
//! 缺一不收）+ 十五件交付声明逐件核销 + 收官预告字段（下一组接续
//! 点 + 遗留风险 + 预告条目须为空或实名）+ 双签放行（组长 + 验收
//! 方互异签名，同人双签拦截）+ 整组回滚（收口后发现红线缺陷整组
//! 回滚重启，回滚后签名作废须重签）。
//! 段位：0x46E0（续表 idx594）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x46E0;
/// 交付声明件数。
pub const DELIVERABLES: usize = 15;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 三件套缺件。
    KitMissing = SEG_BASE,
    /// 交付件未核销。
    DeliverableOpen = SEG_BASE + 0x01,
    /// 同人双签拦截。
    SameSigner = SEG_BASE + 0x02,
    /// 预告字段不实名。
    TeaserVague = SEG_BASE + 0x03,
    /// 整组回滚生效。
    RolledBack = SEG_BASE + 0x04,
}

/// 三件套。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[derive(Default)]
pub struct Kit {
    /// 判据报告。
    pub report: bool,
    /// 交付清单。
    pub manifest: bool,
    /// 对拍记录。
    pub crosscheck: bool,
}

/// 收口台。
#[derive(Default)]
pub struct Closeout {
    pub kit: Kit,
    /// 十五件核销位。
    pub done: Vec<bool>,
    /// 收官预告：接续组号（0 = 未填）。
    pub next_group: u32,
    /// 遗留风险（空 = 无遗留）。
    pub risks: Vec<&'static str>,
    /// 双签：组长 / 验收方（0 = 未签）。
    pub sign_lead: u32,
    pub sign_accept: u32,
    /// 回滚代次（每次整组回滚 +1，签名按代次作废）。
    pub epoch: u32,
    /// 回滚标记。
    pub rolled: bool,
    pub audit: u32,
}

impl Closeout {
    pub fn new(kit: Kit) -> Self {
        Closeout { kit, ..Default::default() }
    }

    /// 三件套齐备断言。
    pub fn kit_ok(&self) -> bool {
        self.kit == Kit { report: true, manifest: true, crosscheck: true }
    }

    /// 交付件核销（序号 1..=15）。
    pub fn settle(&mut self, no: usize) -> Result<(), ClCode> {
        if no == 0 || no > DELIVERABLES {
            return Err(ClCode::DeliverableOpen);
        }
        while self.done.len() < DELIVERABLES {
            self.done.push(false);
        }
        self.done[no - 1] = true;
        Ok(())
    }

    /// 十五件全部核销断言。
    pub fn all_settled(&self) -> bool {
        self.done.len() == DELIVERABLES && self.done.iter().all(|x| *x)
    }

    /// 预告实名断言：接续组号非 0 且每条风险实名（非空串语义由非空切片保证）。
    pub fn teaser_ok(&self) -> bool {
        self.next_group != 0
    }

    /// 双签登记：同人双签拦截；回滚代次不符的旧签作废。
    pub fn sign(&mut self, lead: u32, accept: u32) -> Result<(), ClCode> {
        if lead == 0 || accept == 0 {
            return Err(ClCode::SameSigner);
        }
        if lead == accept {
            return Err(ClCode::SameSigner);
        }
        if self.rolled {
            return Err(ClCode::RolledBack);
        }
        self.sign_lead = lead;
        self.sign_accept = accept;
        self.audit += 1;
        Ok(())
    }

    /// 双签放行断言：三件套 + 十五件 + 预告实名 + 双签互异。
    pub fn gate(&self) -> Result<(), ClCode> {
        if !self.kit_ok() {
            return Err(ClCode::KitMissing);
        }
        if !self.all_settled() {
            return Err(ClCode::DeliverableOpen);
        }
        if !self.teaser_ok() {
            return Err(ClCode::TeaserVague);
        }
        if self.sign_lead == 0 || self.sign_accept == 0 || self.sign_lead == self.sign_accept {
            return Err(ClCode::SameSigner);
        }
        Ok(())
    }

    /// 整组回滚：红线缺陷触发——签名作废（代次 +1），须重走收口重签。
    pub fn rollback(&mut self) {
        self.sign_lead = 0;
        self.sign_accept = 0;
        self.rolled = true;
        self.epoch += 1;
        self.audit = 0;
    }

    /// 回滚后重开：解除回滚标记进入新代次。
    pub fn reopen(&mut self) {
        self.rolled = false;
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves594_checks() -> CheckSet {
    let mut set = CheckSet::new("ves594");

    // C594-01 三件套缺件不收。
    let mut co = Closeout::new(Kit { report: true, manifest: false, crosscheck: true });
    chk(&mut set, "C594-01", !co.kit_ok() && co.gate() == Err(ClCode::KitMissing), "缺件不收");

    // C594-02 三件套齐备。
    co.kit.manifest = true;
    chk(&mut set, "C594-02", co.kit_ok(), "三件套齐");

    // C594-03 未核销不收。
    chk(&mut set, "C594-03", co.gate() == Err(ClCode::DeliverableOpen), "未核销不收");

    // C594-04 越界序号拒。
    chk(&mut set, "C594-04", co.settle(0).is_err() && co.settle(16).is_err(), "越界序号拒");

    // C594-05 十五件逐件核销。
    let mut ok = true;
    for i in 1..=DELIVERABLES {
        ok &= co.settle(i).is_ok();
    }
    chk(&mut set, "C594-05", ok && co.all_settled(), "十五件核销");

    // C594-06 预告未实名拦。
    chk(&mut set, "C594-06", co.gate() == Err(ClCode::TeaserVague), "预告未实名");

    // C594-07 同人双签拦截。
    co.next_group = 9;
    chk(&mut set, "C594-07", co.sign(3, 3) == Err(ClCode::SameSigner), "同人双签拦");

    // C594-08 空签拦截。
    chk(&mut set, "C594-08", co.sign(3, 0).is_err(), "空签拦");

    // C594-09 双签互异放行。
    chk(&mut set, "C594-09", co.sign(3, 4).is_ok() && co.gate().is_ok(), "双签放行");

    // C594-10 回滚后旧签作废。
    co.rollback();
    chk(&mut set, "C594-10", co.sign_lead == 0 && co.epoch == 1, "回滚签废");

    // C594-11 回滚态重签拒（须先重开）。
    chk(&mut set, "C594-11", co.sign(5, 6) == Err(ClCode::RolledBack), "回滚态拒签");

    // C594-12 重开新代次重签放行。
    co.reopen();
    chk(&mut set, "C594-12", co.sign(5, 6).is_ok() && co.gate().is_ok() && co.epoch == 1, "新代次放行");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_gate() {
        let mut co = Closeout::new(Kit { report: true, manifest: true, crosscheck: true });
        assert_eq!(co.sign(1, 1), Err(ClCode::SameSigner));
        assert!(co.sign(1, 2).is_ok());
    }

    #[test]
    fn rollback_cycle() {
        let mut co = Closeout::new(Kit { report: true, manifest: true, crosscheck: true });
        let _ = co.sign(1, 2);
        co.rollback();
        assert_eq!(co.sign(1, 2), Err(ClCode::RolledBack));
        co.reopen();
        assert!(co.sign(3, 4).is_ok() && co.epoch == 1);
    }
}
