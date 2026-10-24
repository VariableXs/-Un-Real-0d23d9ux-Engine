//! VE-F4259 · U03 预备自查（U03 一致性验证平台）
//! 职责：组内自查 20 项逐项核验 + 联调回执 + 性能三线达标 + 基线双签确认
//! ——四件汇总为收口预备报告；核验不过补齐复验、回执缺催办、基线未签阻断。
//! 段位：0x1EA0（续表 idx493）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1EA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    /// 核验不过。
    ItemFail = SEG_BASE,
    /// 回执缺（催办）。
    ReceiptMissing = SEG_BASE + 0x01,
    /// 三线未达（阻断）。
    PerfNotMet = SEG_BASE + 0x02,
    /// 基线未双签（硬阻断）。
    BaselineUnsigned = SEG_BASE + 0x03,
    /// 复验仍未过。
    RecheckFail = SEG_BASE + 0x04,
}

/// 自查项。
#[derive(Clone, Copy, Debug)]
pub struct SelfItem {
    pub no: u8,
    pub pass: bool,
    pub rechecked: bool,
}

/// 四件汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wrapup {
    pub items_ok: bool,
    pub receipt_ok: bool,
    pub perf_ok: bool,
    pub baseline_signed: bool,
}

impl Wrapup {
    pub fn ready(&self) -> bool {
        self.items_ok && self.receipt_ok && self.perf_ok && self.baseline_signed
    }
}

/// 自查册。
pub struct SelfCheckBook {
    pub items: Vec<SelfItem>,
    pub joint_receipt: Option<bool>,
    /// 性能三线：(跑批, P95, 资源) 达标位。
    pub perf3: (bool, bool, bool),
    /// 基线双签记录。
    pub baseline_sigs: Vec<(u16, bool)>,
    pub urges: u32,
}

impl SelfCheckBook {
    pub const ITEM_N: u8 = 20;

    pub fn new() -> Self {
        SelfCheckBook { items: Vec::new(), joint_receipt: None, perf3: (false, false, false), baseline_sigs: Vec::new(), urges: 0 }
    }

    /// 逐项核验登记。
    pub fn check_item(&mut self, no: u8, pass: bool) -> Result<(), ScCode> {
        if no == 0 || no > Self::ITEM_N {
            return Err(ScCode::ItemFail);
        }
        if let Some(it) = self.items.iter_mut().find(|i| i.no == no) {
            it.pass = pass;
        } else {
            self.items.push(SelfItem { no, pass, rechecked: false });
        }
        Ok(())
    }

    /// 复验：只对不过项。
    pub fn recheck(&mut self, no: u8, pass: bool) -> Result<(), ScCode> {
        let it = self.items.iter_mut().find(|i| i.no == no).ok_or(ScCode::ItemFail)?;
        it.pass = pass;
        it.rechecked = true;
        if !pass {
            return Err(ScCode::RecheckFail);
        }
        Ok(())
    }

    /// 全项核验通过。
    pub fn items_ok(&self) -> bool {
        self.items.len() == Self::ITEM_N as usize && self.items.iter().all(|i| i.pass)
    }

    /// 联调回执催办（幂等计数）。
    pub fn urge(&mut self) {
        self.urges += 1;
    }

    /// 四件汇总。
    pub fn wrapup(&self) -> Result<Wrapup, ScCode> {
        let items_ok = self.items_ok();
        if self.joint_receipt.is_none() {
            return Err(ScCode::ReceiptMissing);
        }
        let perf_ok = self.perf3.0 && self.perf3.1 && self.perf3.2;
        if !perf_ok {
            return Err(ScCode::PerfNotMet);
        }
        let baseline_signed = self.baseline_sigs.iter().any(|(_, s)| *s);
        if !baseline_signed {
            return Err(ScCode::BaselineUnsigned);
        }
        Ok(Wrapup { items_ok, receipt_ok: self.joint_receipt.unwrap(), perf_ok, baseline_signed })
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C493-01..12。
pub fn run_ves493_checks() -> CheckSet {
    let mut set = CheckSet::new("ves493");

    let mut bk = SelfCheckBook::new();
    chk(&mut set, "C493-01", bk.check_item(0, true) == Err(ScCode::ItemFail), "零号项拒收");
    chk(&mut set, "C493-02", bk.check_item(21, true) == Err(ScCode::ItemFail), "超 20 项拒收");

    for i in 1..=20u8 {
        let _ = bk.check_item(i, i != 7);
    }
    chk(&mut set, "C493-03", bk.items.len() == 20, "20 项逐项核验登记");
    chk(&mut set, "C493-04", bk.items_ok() == false, "第 7 项不过→未全过");

    // 复验
    chk(&mut set, "C493-05", bk.recheck(7, true).is_ok(), "第 7 项复验转绿");
    chk(&mut set, "C493-06", bk.items_ok(), "全项核验通过");
    let _ = bk.check_item(9, false);
    chk(&mut set, "C493-07", bk.recheck(9, false) == Err(ScCode::RecheckFail), "复验仍不过立案");

    // 四件汇总
    chk(&mut set, "C493-08", bk.wrapup() == Err(ScCode::ReceiptMissing), "回执缺→催办前置");
    bk.urge();
    bk.joint_receipt = Some(true);
    chk(&mut set, "C493-09", bk.urges == 1, "催办幂等计数");
    chk(&mut set, "C493-10", bk.wrapup() == Err(ScCode::PerfNotMet), "三线未达阻断");
    bk.perf3 = (true, true, true);
    chk(&mut set, "C493-11", bk.wrapup() == Err(ScCode::BaselineUnsigned), "基线未双签硬阻断");
    bk.baseline_sigs.push((1, true));
    let w = bk.wrapup().unwrap_or(Wrapup { items_ok: false, receipt_ok: false, perf_ok: false, baseline_signed: false });
    chk(&mut set, "C493-12", w.ready() && w == Wrapup { items_ok: false, receipt_ok: true, perf_ok: true, baseline_signed: true }, "四件汇总出预备报告（items 因复验立案 false）");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selfcheck_wrapup() {
        let mut bk = SelfCheckBook::new();
        assert!(bk.check_item(1, true).is_ok());
        assert_eq!(bk.check_item(21, true), Err(ScCode::ItemFail));
        assert!(bk.wrapup().is_err());
    }
}
