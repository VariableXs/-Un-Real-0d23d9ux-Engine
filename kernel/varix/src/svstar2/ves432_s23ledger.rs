//! ves432_s23ledger — VE-F4192 · T 域清账确认
//! 确认：T 域清账——终审项逐条对账（F4181-F4191 终审回执齐备核验+遗留项清单处置（销项/转 U 域/登记缓办三路））
//! +账实相符声明；回执缺→催办；遗留未处置→禁收官；账实不符→重盘；
//! 遗留清单含多语言无障碍专项路（转 U 域一致性校验——域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x12A0;
pub const FINAL_ITEMS: usize = 11; // F4181-F4191

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LgCode {
    Ok = 0,
    ReceiptMissing = SEG_BASE + 0x01, // 0x12A1
    Untreated = SEG_BASE + 0x02,      // 0x12A2
    Mismatch = SEG_BASE + 0x03,       // 0x12A3
}

/// 三路处置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposal { Close = 0, ToUDomain = 1, Deferred = 2 }

/// 遗留项（含多语言无障碍专项标记——域本色）。
pub struct LegacyItem { pub id: u32, pub a11y_path: bool, pub disposed: Option<Disposal> }

/// 清账册。
pub struct LedgerBook {
    pub receipts: Vec<u32>,           // 终审项回执（项号 1..=11）
    pub urged: u32,
    pub legacy: Vec<LegacyItem>,
    pub reaudits: u32,
}

impl LedgerBook {
    pub fn new() -> Self { LedgerBook { receipts: Vec::new(), urged: 0, legacy: Vec::new(), reaudits: 0 } }
    pub fn put_receipt(&mut self, item: u32) { if !self.receipts.contains(&item) { self.receipts.push(item); } }
    pub fn missing_receipts(&self) -> Vec<u32> {
        (1..=FINAL_ITEMS as u32).filter(|i| !self.receipts.contains(i)).collect()
    }
    /// 回执催办（幂等计数）。
    pub fn urge(&mut self) -> bool {
        if self.missing_receipts().is_empty() { false } else { self.urged += 1; true }
    }
    pub fn add_legacy(&mut self, id: u32, a11y: bool) {
        self.legacy.push(LegacyItem { id, a11y_path: a11y, disposed: None });
    }
    pub fn dispose(&mut self, id: u32, d: Disposal) -> bool {
        for it in self.legacy.iter_mut() {
            if it.id == id { it.disposed = Some(d); return true; }
        }
        false
    }
    /// 遗留处置核验：未处置→禁收官。
    pub fn legacy_clear(&self) -> Result<(), LgCode> {
        if self.legacy.iter().any(|i| i.disposed.is_none()) { return Err(LgCode::Untreated); }
        // 多语言无障碍专项路必须走「转 U 域」并在 U 域做一致性校验（域本色）。
        for it in &self.legacy {
            if it.a11y_path && it.disposed != Some(Disposal::ToUDomain) { return Err(LgCode::Mismatch); }
        }
        Ok(())
    }
    /// 回执齐备核验。
    pub fn receipts_full(&self) -> Result<(), LgCode> {
        if self.missing_receipts().is_empty() { Ok(()) } else { Err(LgCode::ReceiptMissing) }
    }
    /// 账实相符声明：回执齐+遗留清+账面数与实际项数一致。
    pub fn reconcile(&self, declared: usize) -> Result<(), LgCode> {
        self.receipts_full()?;
        self.legacy_clear()?;
        if declared != self.receipts.len() { return Err(LgCode::Mismatch); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves432_checks() -> CheckSet {
    let mut set = CheckSet::new("ves432");
    let mut bk = LedgerBook::new();

    // C432-01 回执缺→催办。
    for i in 1..=8u32 { bk.put_receipt(i); }
    chk(&mut set, "C432-01", bk.urge() && bk.urged == 1, "回执缺催办");

    // C432-02 缺项定位（9/10/11）。
    chk(&mut set, "C432-02", bk.missing_receipts() == vec![9, 10, 11], "缺项定位");

    // C432-03 回执齐后催办停。
    for i in 9..=11u32 { bk.put_receipt(i); }
    chk(&mut set, "C432-03", !bk.urge() && bk.receipts_full().is_ok(), "回执齐");

    // C432-04 遗留未处置→禁收官。
    bk.add_legacy(501, false);
    bk.add_legacy(502, true); // 多语言无障碍专项（域本色）
    chk(&mut set, "C432-04", bk.legacy_clear() == Err(LgCode::Untreated), "未处置禁收官");

    // C432-05 销项处置单条。
    bk.dispose(501, Disposal::Close);
    chk(&mut set, "C432-05", bk.dispose(502, Disposal::Close)
        && bk.legacy_clear() == Err(LgCode::Mismatch), "a11y 项错路");

    // C432-06 多语言无障碍专项路必须转 U 域（域本色一致性校验）。
    bk.dispose(502, Disposal::ToUDomain);
    chk(&mut set, "C432-06", bk.legacy_clear().is_ok(), "专项路转 U 域");

    // C432-07 账实相符：声明 11 与实收 11 一致。
    chk(&mut set, "C432-07", bk.reconcile(11).is_ok(), "账实相符");

    // C432-08 账实不符→重盘码。
    chk(&mut set, "C432-08", bk.reconcile(10) == Err(LgCode::Mismatch), "账实不符");

    // C432-09 回执幂等。
    bk.put_receipt(9);
    chk(&mut set, "C432-09", bk.receipts.len() == 11, "回执幂等");

    // C432-10 遗项不存在处置失败。
    chk(&mut set, "C432-10", !bk.dispose(999, Disposal::Close), "无此遗项");

    // C432-11 三路单源断言。
    chk(&mut set, "C432-11", Disposal::Close as u8 == 0
        && Disposal::ToUDomain as u8 == 1 && Disposal::Deferred as u8 == 2, "三路单源");

    // C432-12 清账终态：全部条件再核。
    chk(&mut set, "C432-12", bk.reconcile(11).is_ok() && bk.missing_receipts().is_empty()
        && bk.legacy.iter().all(|i| i.disposed.is_some()), "清账终态");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves432_smoke() {
        let set = run_ves432_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves432 red: {:?}", red);
    }
}
