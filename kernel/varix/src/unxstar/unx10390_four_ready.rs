//! UNX-F10390 四项齐备核（批册/深化册/handoff/台账四件套核）（0x1bb40 段）
//! 锚点：UNX-F10390-J1 四件套齐备核（批册/深化册/handoff/台账逐件存在且非空）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bb40;
pub const EV_NO_BATCH: u32 = SEG_BASE + 1;
pub const EV_NO_DEEP: u32 = SEG_BASE + 2;
pub const EV_NO_HANDOFF: u32 = SEG_BASE + 3;
pub const EV_NO_LEDGER: u32 = SEG_BASE + 4;
pub const EV_SEALED: u32 = SEG_BASE + 5;
pub const EV_TWICE: u32 = SEG_BASE + 6;

pub const SUITE_ANCHOR: usize = 4;

/// 单件套：名 + 存在标记 + 体量（行数）
#[derive(Clone, Debug)]
pub struct SuiteItem {
    pub name: &'static str,
    pub present: bool,
    pub lines: usize,
}

/// 四件套核：四件齐备（存在 + 非空）方可出 PASS
pub struct SuiteCheck {
    items: [Option<SuiteItem>; SUITE_ANCHOR],
    sealed: bool,
    passed: Option<bool>,
}

const NONE_ITEM: Option<SuiteItem> = None;

impl SuiteCheck {
    pub fn new() -> Self {
        SuiteCheck { items: [NONE_ITEM; SUITE_ANCHOR], sealed: false, passed: None }
    }

    /// 槽位 0=批册 1=深化册 2=handoff 3=台账
    pub fn put(&mut self, slot: usize, item: SuiteItem) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if slot >= SUITE_ANCHOR {
            return Err(EV_NO_BATCH);
        }
        if self.items[slot].is_some() {
            return Err(EV_TWICE);
        }
        self.items[slot] = Some(item);
        Ok(())
    }

    fn missing_code(slot: usize) -> u32 {
        match slot {
            0 => EV_NO_BATCH,
            1 => EV_NO_DEEP,
            2 => EV_NO_HANDOFF,
            _ => EV_NO_LEDGER,
        }
    }

    /// 齐备核：四件全存在且非空
    pub fn examine(&mut self) -> Result<bool, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        let mut i = 0;
        while i < SUITE_ANCHOR {
            match &self.items[i] {
                None => {
                    self.sealed = true;
                    self.passed = Some(false);
                    return Err(Self::missing_code(i));
                }
                Some(it) => {
                    if !it.present || it.lines == 0 {
                        self.sealed = true;
                        self.passed = Some(false);
                        return Err(Self::missing_code(i));
                    }
                }
            }
            i += 1;
        }
        self.sealed = true;
        self.passed = Some(true);
        Ok(true)
    }

    pub fn result_of(&self) -> Option<bool> {
        self.passed
    }

    pub fn present_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < SUITE_ANCHOR {
            if let Some(it) = &self.items[i] {
                if it.present && it.lines > 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn total_lines(&self) -> usize {
        let mut acc = 0;
        let mut i = 0;
        while i < SUITE_ANCHOR {
            if let Some(it) = &self.items[i] {
                acc += it.lines;
            }
            i += 1;
        }
        acc
    }

    pub fn slot_name(&self, slot: usize) -> Option<&'static str> {
        if slot >= SUITE_ANCHOR {
            return None;
        }
        match &self.items[slot] {
            Some(it) => Some(it.name),
            None => None,
        }
    }
}

/// 满套构造：四件齐备各挂体量
fn full_suite(batch: usize, deep: usize, handoff: usize, ledger: usize) -> SuiteCheck {
    let mut sc = SuiteCheck::new();
    sc.put(0, SuiteItem { name: "批册", present: true, lines: batch }).unwrap();
    sc.put(1, SuiteItem { name: "深化册", present: true, lines: deep }).unwrap();
    sc.put(2, SuiteItem { name: "handoff", present: true, lines: handoff }).unwrap();
    sc.put(3, SuiteItem { name: "台账", present: true, lines: ledger }).unwrap();
    sc
}

pub fn run_unx10390_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10390");
    s.add("J1-段锚", SEG_BASE == 0x1bb40, "四件套核段基址 0x1bb40");
    s.add("J2-件数锚", SUITE_ANCHOR == 4, "四件套锚");
    s.add("J3-新册零齐备", SuiteCheck::new().present_count() == 0, "新册 0/4");
    s.add("J4-满套齐备", full_suite(1200, 800, 300, 500).present_count() == 4, "满套 4/4");
    s.add("J5-满套过检", {
        let mut sc = full_suite(1200, 800, 300, 500);
        sc.examine() == Ok(true)
    }, "四件齐备过检 true");
    s.add("J6-缺批册报码", {
        let mut sc = SuiteCheck::new();
        sc.put(1, SuiteItem { name: "深化册", present: true, lines: 800 }).unwrap();
        sc.examine().unwrap_err() == EV_NO_BATCH
    }, "缺槽 0 EV_NO_BATCH");
    s.add("J7-缺深化册报码", {
        let mut sc = full_suite(1200, 0, 300, 500);
        sc.examine().unwrap_err() == EV_NO_DEEP
    }, "深化册 0 行 EV_NO_DEEP");
    s.add("J8-缺 handoff 报码", {
        let mut sc = full_suite(1200, 800, 0, 500);
        sc.examine().unwrap_err() == EV_NO_HANDOFF
    }, "handoff 0 行 EV_NO_HANDOFF");
    s.add("J9-缺台账报码", {
        let mut sc = full_suite(1200, 800, 300, 0);
        sc.examine().unwrap_err() == EV_NO_LEDGER
    }, "台账 0 行 EV_NO_LEDGER");
    s.add("J10-缺席标记", {
        let mut sc = full_suite(1200, 800, 300, 500);
        sc.put(2, SuiteItem { name: "x", present: false, lines: 1 }).unwrap_err() == EV_TWICE
    }, "槽位重复 EV_TWICE");
    s.add("J11-槽位越界", {
        let mut sc = SuiteCheck::new();
        sc.put(4, SuiteItem { name: "x", present: true, lines: 1 }).unwrap_err() == EV_NO_BATCH
    }, "槽 4 越界拒收");
    s.add("J12-过检封账", {
        let mut sc = full_suite(1200, 800, 300, 500);
        sc.examine().unwrap();
        sc.examine().unwrap_err() == EV_SEALED
    }, "过检一次即封账");
    s.add("J13-结果存档真", {
        let mut sc = full_suite(1200, 800, 300, 500);
        sc.examine().unwrap();
        sc.result_of() == Some(true)
    }, "PASS 存档 true");
    s.add("J14-结果存档假", {
        let mut sc = SuiteCheck::new();
        let _ = sc.examine();
        sc.result_of() == Some(false)
    }, "FAIL 存档 false");
    s.add("J15-槽名可读", {
        let sc = full_suite(1200, 800, 300, 500);
        sc.slot_name(0) == Some("批册") && sc.slot_name(3) == Some("台账")
    }, "槽 0 批册 / 槽 3 台账");
    s.add("J16-体量总账", {
        let sc = full_suite(1200, 800, 300, 500);
        sc.total_lines() == 2_800
    }, "四件体量和 2,800");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_suite_pass() {
        let mut sc = full_suite(1200, 800, 300, 500);
        assert_eq!(sc.examine().unwrap(), true);
        assert_eq!(sc.result_of(), Some(true));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10390_checks();
        assert!(s.all_passed(), "UNX-F10390 判据全绿");
    }
}
