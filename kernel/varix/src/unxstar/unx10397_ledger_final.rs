//! UNX-F10397 台账终写（根台账四处终态）（0x1bc20 段）
//! 锚点：UNX-F10397-J1 根台账终写（四处槽位逐槽终态+总核一致）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bc20;
pub const EV_SLOT_TWICE: u32 = SEG_BASE + 1;
pub const EV_SEALED: u32 = SEG_BASE + 2;
pub const EV_NOT_FOUR: u32 = SEG_BASE + 3;
pub const EV_STATE_DRIFT: u32 = SEG_BASE + 4;
pub const EV_RANGE_OUT: u32 = SEG_BASE + 5;
pub const EV_VALUE_ZERO: u32 = SEG_BASE + 6;

pub const SLOT_ANCHOR: usize = 4;

/// 台账槽位 1..=4：B31~B40 / 判据库 / 缺陷账 / 根哈希
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    BatchTable,
    CriteriaLib,
    DefectLedger,
    RootHash,
}

impl Slot {
    pub fn idx(&self) -> usize {
        match self {
            Slot::BatchTable => 0,
            Slot::CriteriaLib => 1,
            Slot::DefectLedger => 2,
            Slot::RootHash => 3,
        }
    }
}

/// 台账终写器：四处逐槽登记终值，四处一致口径后终写
pub struct LedgerFinal {
    values: [Option<u64>; SLOT_ANCHOR],
    sealed: bool,
    total: Option<u64>,
}

impl LedgerFinal {
    pub fn new() -> Self {
        LedgerFinal { values: [None; SLOT_ANCHOR], sealed: false, total: None }
    }

    pub fn put(&mut self, slot: Slot, value: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if value == 0 {
            return Err(EV_VALUE_ZERO);
        }
        if self.values[slot.idx()].is_some() {
            return Err(EV_SLOT_TWICE);
        }
        self.values[slot.idx()] = Some(value);
        Ok(())
    }

    pub fn filled_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < SLOT_ANCHOR {
            if self.values[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn value_of(&self, slot: Slot) -> Option<u64> {
        self.values[slot.idx()]
    }

    /// 终写：四处齐备求和入档
    pub fn finalize_write(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.filled_count() != SLOT_ANCHOR {
            self.sealed = true;
            return Err(EV_NOT_FOUR);
        }
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < SLOT_ANCHOR {
            acc = acc.wrapping_add(self.values[i].unwrap());
            i += 1;
        }
        self.total = Some(acc);
        self.sealed = true;
        Ok(acc)
    }

    pub fn total_of(&self) -> Option<u64> {
        self.total
    }

    /// 状态漂移核：指定槽期望值与登记值比对
    pub fn drift_check(&self, slot: Slot, expect: u64) -> Result<(), u32> {
        match self.values[slot.idx()] {
            Some(v) if v == expect => Ok(()),
            Some(_) => Err(EV_STATE_DRIFT),
            None => Err(EV_RANGE_OUT),
        }
    }
}

pub fn run_unx10397_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10397");
    s.add("J1-段锚", SEG_BASE == 0x1bc20, "台账终写段基址 0x1bc20");
    s.add("J2-槽位锚", SLOT_ANCHOR == 4, "四处锚");
    s.add("J3-槽位映射", {
        Slot::BatchTable.idx() == 0
            && Slot::CriteriaLib.idx() == 1
            && Slot::DefectLedger.idx() == 2
            && Slot::RootHash.idx() == 3
    }, "四槽 0..=3");
    s.add("J4-新册零槽", {
        let lf = LedgerFinal::new();
        lf.filled_count() == 0
    }, "新册 0/4");
    s.add("J5-逐槽登记", {
        let mut lf = LedgerFinal::new();
        lf.put(Slot::BatchTable, 40).unwrap();
        lf.put(Slot::CriteriaLib, 800).unwrap();
        lf.put(Slot::DefectLedger, 512).unwrap();
        lf.put(Slot::RootHash, 0xdead_beef).unwrap();
        lf.filled_count() == 4
    }, "四槽齐填 4/4");
    s.add("J6-零值拒收", {
        let mut lf = LedgerFinal::new();
        lf.put(Slot::BatchTable, 0).unwrap_err() == EV_VALUE_ZERO
    }, "0 值 EV_VALUE_ZERO");
    s.add("J7-重槽拒收", {
        let mut lf = LedgerFinal::new();
        lf.put(Slot::BatchTable, 40).unwrap();
        lf.put(Slot::BatchTable, 41).unwrap_err() == EV_SLOT_TWICE
    }, "重槽 EV_SLOT_TWICE");
    s.add("J8-终写求和", {
        let mut lf = LedgerFinal::new();
        lf.put(Slot::BatchTable, 40).unwrap();
        lf.put(Slot::CriteriaLib, 800).unwrap();
        lf.put(Slot::DefectLedger, 512).unwrap();
        lf.put(Slot::RootHash, 648).unwrap();
        lf.finalize_write() == Ok(2_000)
    }, "40+800+512+648=2,000");
    s.add("J9-总量存档", {
        let mut lf = LedgerFinal::new();
        lf.put(Slot::BatchTable, 10).unwrap();
        lf.put(Slot::CriteriaLib, 20).unwrap();
        lf.put(Slot::DefectLedger, 30).unwrap();
        lf.put(Slot::RootHash, 40).unwrap();
        lf.finalize_write().unwrap();
        lf.total_of() == Some(100)
    }, "总量存档 100");
    s.add("J10-缺槽拒写", {
        let mut lf = LedgerFinal::new();
        lf.put(Slot::BatchTable, 40).unwrap();
        lf.finalize_write().unwrap_err() == EV_NOT_FOUR
    }, "一槽 EV_NOT_FOUR");
    s.add("J11-终写封账", {
        let mut lf = LedgerFinal::new();
        lf.put(Slot::BatchTable, 1).unwrap();
        lf.put(Slot::CriteriaLib, 2).unwrap();
        lf.put(Slot::DefectLedger, 3).unwrap();
        lf.put(Slot::RootHash, 4).unwrap();
        lf.finalize_write().unwrap();
        lf.put(Slot::RootHash, 9).unwrap_err() == EV_SEALED
    }, "终写后入槽 EV_SEALED");
    s.add("J12-漂移核对平", {
        let mut lf = LedgerFinal::new();
        lf.put(Slot::CriteriaLib, 800).unwrap();
        lf.drift_check(Slot::CriteriaLib, 800).is_ok()
    }, "期望 800 核对平 Ok");
    s.add("J13-漂移核报漂", {
        let mut lf = LedgerFinal::new();
        lf.put(Slot::CriteriaLib, 799).unwrap();
        lf.drift_check(Slot::CriteriaLib, 800) == Err(EV_STATE_DRIFT)
    }, "期望 800 实 799 EV_STATE_DRIFT");
    s.add("J14-漂移核空槽", {
        let lf = LedgerFinal::new();
        lf.drift_check(Slot::RootHash, 1) == Err(EV_RANGE_OUT)
    }, "空槽 EV_RANGE_OUT");
    s.add("J15-值可回读", {
        let mut lf = LedgerFinal::new();
        lf.put(Slot::RootHash, 0x1234).unwrap();
        lf.value_of(Slot::RootHash) == Some(0x1234)
    }, "槽值回读 0x1234");
    s.add("J16-四槽全回读", {
        let mut lf = LedgerFinal::new();
        lf.put(Slot::BatchTable, 40).unwrap();
        lf.put(Slot::CriteriaLib, 800).unwrap();
        lf.put(Slot::DefectLedger, 512).unwrap();
        lf.put(Slot::RootHash, 648).unwrap();
        lf.value_of(Slot::BatchTable) == Some(40)
            && lf.value_of(Slot::CriteriaLib) == Some(800)
            && lf.value_of(Slot::DefectLedger) == Some(512)
            && lf.value_of(Slot::RootHash) == Some(648)
    }, "四槽逐槽回读全中");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_ledger_final() {
        let mut lf = LedgerFinal::new();
        lf.put(Slot::BatchTable, 40).unwrap();
        lf.put(Slot::CriteriaLib, 800).unwrap();
        lf.put(Slot::DefectLedger, 512).unwrap();
        lf.put(Slot::RootHash, 648).unwrap();
        assert_eq!(lf.finalize_write().unwrap(), 2_000);
    }

    #[test]
    fn checks_green() {
        let s = run_unx10397_checks();
        assert!(s.all_passed(), "UNX-F10397 判据全绿");
    }
}
