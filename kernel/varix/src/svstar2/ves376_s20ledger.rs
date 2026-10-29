//! ves376_s20ledger — VE-F4136 · T07 i18n 遗留清账
//! 性能遗留项清单三类（历史劣化未决/预算豁免未销/校准未做）×处置三路（销项/转 T08/登记缓办）+ 账实相符声明 + 遗留未处置禁双签 + 豁免到期强制重议 + 无障碍预算豁免专项到期必销（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xDA40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LdCode {
    Ok = 0,
    BlockSign = SEG_BASE + 0x01,
    ExpiryDue = SEG_BASE + 0x02,
    LedgerMismatch = SEG_BASE + 0x03,
}

/// 三类遗留。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LegacyKind { RegressionOpen = 0, WaiverOpen = 1, CalibMissing = 2 }

impl LegacyKind {
    pub const ALL: [LegacyKind; 3] = [
        LegacyKind::RegressionOpen, LegacyKind::WaiverOpen, LegacyKind::CalibMissing];
    pub fn name(self) -> &'static str {
        match self { LegacyKind::RegressionOpen => "regression-open",
            LegacyKind::WaiverOpen => "waiver-open", LegacyKind::CalibMissing => "calib-missing" }
    }
}

/// 处置三路。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition { Close = 0, ToT08 = 1, Deferred = 2 }

impl Disposition {
    pub const ALL: [Disposition; 3] = [Disposition::Close, Disposition::ToT08, Disposition::Deferred];
    pub fn name(self) -> &'static str {
        match self { Disposition::Close => "close", Disposition::ToT08 => "to-t08", Disposition::Deferred => "deferred" }
    }
}

/// 遗留条目。
pub struct LegacyItem {
    pub kind: LegacyKind,
    pub what: &'static str,
    /// 预算豁免到期轮次（0 = 无豁免时限）。
    pub waiver_rounds_left: u32,
    /// 无障碍预算豁免专项（到期必销——域本色）。
    pub a11y_waiver: bool,
    pub done: Option<Disposition>,
}

pub const WAIVER_MAX_ROUNDS: u32 = 3;

pub struct LegacyBook { pub items: Vec<LegacyItem> }

impl LegacyBook {
    pub fn new() -> Self { LegacyBook { items: Vec::new() } }

    pub fn add(&mut self, it: LegacyItem) { self.items.push(it); }

    /// 豁免到期 → 强制重议（返回到期清单）。
    pub fn expiries(&self) -> Vec<&'static str> {
        self.items.iter()
            .filter(|i| i.waiver_rounds_left > 0 && i.waiver_rounds_left <= 1
                || i.waiver_rounds_left == 0 && i.done.is_none() && i.kind == LegacyKind::WaiverOpen && i.a11y_waiver)
            .map(|i| i.what)
            .collect()
    }

    /// 处置：三路之一。
    pub fn dispose(&mut self, what: &str, d: Disposition) -> Result<(), LdCode> {
        match self.items.iter_mut().find(|i| i.what == what) {
            Some(i) => { i.done = Some(d); Ok(()) }
            None => Err(LdCode::LedgerMismatch),
        }
    }

    /// 未处置清单。
    pub fn open_items(&self) -> Vec<&'static str> {
        self.items.iter().filter(|i| i.done.is_none()).map(|i| i.what).collect()
    }

    /// 禁双签门：任何遗留未处置 → 拒。
    pub fn sign_gate(&self) -> Result<(), LdCode> {
        if self.open_items().is_empty() { Ok(()) } else { Err(LdCode::BlockSign) }
    }

    /// 无障碍专项必销（域本色）：a11y 豁免不允许 deferred。
    pub fn a11y_closed(&self) -> bool {
        self.items.iter().filter(|i| i.a11y_waiver)
            .all(|i| i.done == Some(Disposition::Close))
    }
}

/// 账实相符声明：清单条目数 == 实际遗留数。
pub fn ledger_matches(book: &LegacyBook, actual_count: usize) -> bool {
    book.items.len() == actual_count
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves376_checks() -> CheckSet {
    let mut set = CheckSet::new("ves376");
    let mut bk = LegacyBook::new();

    // C376-01 三类遗留登记。
    bk.add(LegacyItem { kind: LegacyKind::RegressionOpen, what: "cjk-layout-slow", waiver_rounds_left: 0, a11y_waiver: false, done: None });
    bk.add(LegacyItem { kind: LegacyKind::WaiverOpen, what: "arabic-budget-waiver", waiver_rounds_left: 2, a11y_waiver: false, done: None });
    bk.add(LegacyItem { kind: LegacyKind::CalibMissing, what: "thai-calib", waiver_rounds_left: 0, a11y_waiver: false, done: None });
    chk(&mut set, "C376-01", bk.items.len() == 3, "三类各一");

    // C376-02 账实相符。
    chk(&mut set, "C376-02", ledger_matches(&bk, 3), "账 3 = 实 3");

    // C376-03 账实不符检出。
    chk(&mut set, "C376-03", !ledger_matches(&bk, 5), "实 5 ≠ 账 3");

    // C376-04 未处置禁双签。
    chk(&mut set, "C376-04", bk.sign_gate() == Err(LdCode::BlockSign), "3 项未销 => 禁签");

    // C376-05 三路处置：销项。
    let _ = bk.dispose("thai-calib", Disposition::Close);
    chk(&mut set, "C376-05", !bk.open_items().contains(&"thai-calib"), "校准销项");

    // C376-06 转 T08。
    let _ = bk.dispose("cjk-layout-slow", Disposition::ToT08);
    chk(&mut set, "C376-06", bk.open_items() == vec!["arabic-budget-waiver"], "转办后余一项");

    // C376-07 豁免到期强制重议（剩 1 轮即到期线）。
    let mut bk2 = LegacyBook::new();
    bk2.add(LegacyItem { kind: LegacyKind::WaiverOpen, what: "soon-expire", waiver_rounds_left: 1, a11y_waiver: false, done: None });
    chk(&mut set, "C376-07", bk2.expiries() == vec!["soon-expire"], "剩 1 轮 → 重议");

    // C376-08 未到期满不在列。
    let mut bk3 = LegacyBook::new();
    bk3.add(LegacyItem { kind: LegacyKind::WaiverOpen, what: "far", waiver_rounds_left: 3, a11y_waiver: false, done: None });
    chk(&mut set, "C376-08", bk3.expiries().is_empty(), "3 轮未到期");

    // C376-09 无障碍豁免专项必销（域本色）：deferred 不认。
    let mut bk4 = LegacyBook::new();
    bk4.add(LegacyItem { kind: LegacyKind::WaiverOpen, what: "a11y-waiver-x", waiver_rounds_left: 0, a11y_waiver: true, done: None });
    let _ = bk4.dispose("a11y-waiver-x", Disposition::Deferred);
    chk(&mut set, "C376-09", !bk4.a11y_closed(), "缓办不算销");

    // C376-10 专项销项后过。
    let _ = bk4.dispose("a11y-waiver-x", Disposition::Close);
    chk(&mut set, "C376-10", bk4.a11y_closed(), "销项后专项清");

    // C376-11 全销后过签门。
    let _ = bk.dispose("arabic-budget-waiver", Disposition::Close);
    chk(&mut set, "C376-11", bk.open_items().is_empty() && bk.sign_gate().is_ok(), "全销 → 可签");

    // C376-12 三路名单单源。
    chk(&mut set, "C376-12",
        Disposition::ALL.map(|d| d.name()) == ["close", "to-t08", "deferred"], "三路单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves376_smoke() {
        let set = run_ves376_checks();
        assert!(set.failed_count() == 0, "ves376 red: {}", set.red_items_text());
    }

    #[test]
    fn ves376_gate() {
        let mut b = LegacyBook::new();
        assert!(b.sign_gate().is_ok());
        b.add(LegacyItem { kind: LegacyKind::CalibMissing, what: "x", waiver_rounds_left: 0, a11y_waiver: false, done: None });
        assert_eq!(b.sign_gate(), Err(LdCode::BlockSign));
    }
}
