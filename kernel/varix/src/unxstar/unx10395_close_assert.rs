//! UNX-F10395 F10400 域收官断言（域终结单值断言成文）（0x1bbe0 段）
//! 锚点：UNX-F10395-J1 域收官断言（单值 CLOSED 成文+前置五源齐备）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bbe0;
pub const EV_SRC_MISSING: u32 = SEG_BASE + 1;
pub const EV_ALREADY: u32 = SEG_BASE + 2;
pub const EV_PREMATURE: u32 = SEG_BASE + 3;
pub const EV_SEALED: u32 = SEG_BASE + 4;
pub const EV_BAD_ASSERT: u32 = SEG_BASE + 5;
pub const EV_NO_TEXT: u32 = SEG_BASE + 6;

pub const SRC_ANCHOR: usize = 5;
pub const CLOSED: &str = "CLOSED";

/// 前置五源：陈述书 / 根哈希 / 四件套 / 缺陷账 / 守恒核
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SrcKind {
    Stmt,
    RootHash,
    Suite,
    Defect,
    Conserv,
}

impl SrcKind {
    pub fn slot(&self) -> usize {
        match self {
            SrcKind::Stmt => 0,
            SrcKind::RootHash => 1,
            SrcKind::Suite => 2,
            SrcKind::Defect => 3,
            SrcKind::Conserv => 4,
        }
    }
}

/// 收官断言器：五源登记齐备后方可单值断言 CLOSED
pub struct CloseAssert {
    srcs: [bool; SRC_ANCHOR],
    text: Option<String>,
    asserted: bool,
    sealed: bool,
}

impl CloseAssert {
    pub fn new() -> Self {
        CloseAssert { srcs: [false; SRC_ANCHOR], text: None, asserted: false, sealed: false }
    }

    pub fn mark(&mut self, k: SrcKind) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        self.srcs[k.slot()] = true;
        Ok(())
    }

    pub fn marked_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < SRC_ANCHOR {
            if self.srcs[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn missing_slots(&self) -> Vec<usize> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < SRC_ANCHOR {
            if !self.srcs[i] {
                v.push(i);
            }
            i += 1;
        }
        v
    }

    /// 成文：五源齐 + 文本含 CLOSED 单值
    pub fn assert_closed(&mut self, text: &str) -> Result<&'static str, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.asserted {
            return Err(EV_ALREADY);
        }
        if text.is_empty() || !text.contains(CLOSED) {
            self.sealed = true;
            return Err(EV_BAD_ASSERT);
        }
        if self.marked_count() != SRC_ANCHOR {
            self.sealed = true;
            return Err(EV_PREMATURE);
        }
        self.text = Some(text.to_string());
        self.asserted = true;
        self.sealed = true;
        Ok(CLOSED)
    }

    pub fn text_of(&self) -> Option<&str> {
        match &self.text {
            Some(t) => Some(t.as_str()),
            None => None,
        }
    }

    pub fn is_asserted(&self) -> bool {
        self.asserted
    }
}

pub fn run_unx10395_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10395");
    s.add("J1-段锚", SEG_BASE == 0x1bbe0, "收官断言段基址 0x1bbe0");
    s.add("J2-源数锚", SRC_ANCHOR == 5, "前置五源锚");
    s.add("J3-单值锚", CLOSED == "CLOSED", "终结单值 CLOSED");
    s.add("J4-新册零源", {
        let a = CloseAssert::new();
        a.marked_count() == 0 && a.missing_slots() == vec![0, 1, 2, 3, 4]
    }, "新册缺槽 0..=4");
    s.add("J5-槽位映射", {
        SrcKind::Stmt.slot() == 0
            && SrcKind::RootHash.slot() == 1
            && SrcKind::Suite.slot() == 2
            && SrcKind::Defect.slot() == 3
            && SrcKind::Conserv.slot() == 4
    }, "五源槽位 0..=4");
    s.add("J6-断言过早", {
        let mut a = CloseAssert::new();
        a.mark(SrcKind::Stmt).unwrap();
        a.assert_closed("CLOSED now").unwrap_err() == EV_PREMATURE
    }, "一源即断言 EV_PREMATURE");
    s.add("J7-文本缺值", {
        let mut a = CloseAssert::new();
        let mut k = 0;
        while k < 5 {
            a.mark(match k {
                0 => SrcKind::Stmt,
                1 => SrcKind::RootHash,
                2 => SrcKind::Suite,
                3 => SrcKind::Defect,
                _ => SrcKind::Conserv,
            }).unwrap();
            k += 1;
        }
        a.assert_closed("not yet").unwrap_err() == EV_BAD_ASSERT
    }, "无 CLOSED 词 EV_BAD_ASSERT");
    s.add("J8-空文本拒", {
        let mut a = CloseAssert::new();
        let mut k = 0;
        while k < 5 {
            a.mark(match k {
                0 => SrcKind::Stmt,
                1 => SrcKind::RootHash,
                2 => SrcKind::Suite,
                3 => SrcKind::Defect,
                _ => SrcKind::Conserv,
            }).unwrap();
            k += 1;
        }
        a.assert_closed("").unwrap_err() == EV_BAD_ASSERT
    }, "空文本 EV_BAD_ASSERT");
    s.add("J9-五源断言成", {
        let mut a = CloseAssert::new();
        let kinds = [SrcKind::Stmt, SrcKind::RootHash, SrcKind::Suite, SrcKind::Defect, SrcKind::Conserv];
        let mut k = 0;
        while k < 5 {
            a.mark(kinds[k]).unwrap();
            k += 1;
        }
        a.assert_closed("域收官 CLOSED @F10400") == Ok(CLOSED)
    }, "五源齐断言 Ok(CLOSED)");
    s.add("J10-成文可读", {
        let mut a = CloseAssert::new();
        let kinds = [SrcKind::Stmt, SrcKind::RootHash, SrcKind::Suite, SrcKind::Defect, SrcKind::Conserv];
        let mut k = 0;
        while k < 5 {
            a.mark(kinds[k]).unwrap();
            k += 1;
        }
        a.assert_closed("CLOSED").unwrap();
        a.text_of() == Some("CLOSED")
    }, "断言文本可回读");
    s.add("J11-重复断言拒", {
        let mut a = CloseAssert::new();
        let kinds = [SrcKind::Stmt, SrcKind::RootHash, SrcKind::Suite, SrcKind::Defect, SrcKind::Conserv];
        let mut k = 0;
        while k < 5 {
            a.mark(kinds[k]).unwrap();
            k += 1;
        }
        a.assert_closed("CLOSED").unwrap();
        a.assert_closed("CLOSED again").unwrap_err() == EV_ALREADY
    }, "二次断言 EV_ALREADY");
    s.add("J12-断言后封账", {
        let mut a = CloseAssert::new();
        let kinds = [SrcKind::Stmt, SrcKind::RootHash, SrcKind::Suite, SrcKind::Defect, SrcKind::Conserv];
        let mut k = 0;
        while k < 5 {
            a.mark(kinds[k]).unwrap();
            k += 1;
        }
        a.assert_closed("CLOSED").unwrap();
        a.mark(SrcKind::Stmt).unwrap_err() == EV_SEALED
    }, "断言后登记 EV_SEALED");
    s.add("J13-过早失败亦封账", {
        let mut a = CloseAssert::new();
        a.mark(SrcKind::Stmt).unwrap();
        a.assert_closed("CLOSED").unwrap_err() == EV_PREMATURE;
        a.mark(SrcKind::RootHash).unwrap_err() == EV_SEALED
    }, "失败断言亦封账");
    s.add("J14-缺槽递减", {
        let mut a = CloseAssert::new();
        a.mark(SrcKind::Stmt).unwrap();
        a.mark(SrcKind::Suite).unwrap();
        a.missing_slots() == vec![1, 3, 4]
    }, "已标 0/2 缺 1/3/4");
    s.add("J15-断言态可查", {
        let mut a = CloseAssert::new();
        !a.is_asserted()
    }, "未断言态 false");
    s.add_dyn("J16-逐源登记计数", {
        let mut a = CloseAssert::new();
        a.mark(SrcKind::Stmt).unwrap();
        let c1 = a.marked_count();
        a.mark(SrcKind::Conserv).unwrap();
        let c2 = a.marked_count();
        c1 == 1 && c2 == 2
    }, "逐源登记 1-2");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_close_assert() {
        let mut a = CloseAssert::new();
        let kinds = [SrcKind::Stmt, SrcKind::RootHash, SrcKind::Suite, SrcKind::Defect, SrcKind::Conserv];
        for k in kinds.iter() {
            a.mark(*k).unwrap();
        }
        assert_eq!(a.assert_closed("域收官 CLOSED"), Ok(CLOSED));
        assert!(a.is_asserted());
    }

    #[test]
    fn checks_green() {
        let s = run_unx10395_checks();
        assert!(s.all_passed(), "UNX-F10395 判据全绿");
    }
}
