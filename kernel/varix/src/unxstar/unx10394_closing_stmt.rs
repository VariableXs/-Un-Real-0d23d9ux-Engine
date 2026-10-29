//! UNX-F10394 域收官陈述书（五节模板域级实例）（0x1bbc0 段）
//! 锚点：UNX-F10394-J1 域收官陈述书（五节齐备+逐节非空+节序单调）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bbc0;
pub const EV_SECTION_EMPTY: u32 = SEG_BASE + 1;
pub const EV_NOT_FIVE: u32 = SEG_BASE + 2;
pub const EV_OUT_OF_ORDER: u32 = SEG_BASE + 3;
pub const EV_SEALED: u32 = SEG_BASE + 4;
pub const EV_DUP_SECTION: u32 = SEG_BASE + 5;
pub const EV_RANGE_OUT: u32 = SEG_BASE + 6;

pub const SECTION_ANCHOR: usize = 5;

/// 五节编号 1..=5：总览 / 判据 / 账本 / 缺陷 / 移交
#[derive(Clone, Debug)]
pub struct Section {
    pub no: usize,
    pub body: String,
}

impl Section {
    pub fn empty(no: usize) -> Self {
        Section { no, body: String::new() }
    }
    pub fn filled(no: usize, body: &str) -> Self {
        Section { no, body: body.to_string() }
    }
    pub fn is_filled(&self) -> bool {
        self.body.len() >= 16
    }
}

/// 陈述书：按节序 1..=5 填写，齐备后封卷
pub struct ClosingStmt {
    sections: Vec<Section>,
    sealed: bool,
    signed: Option<u32>,
}

const FNV_SEED: u32 = 0x811c_9dc5;

impl ClosingStmt {
    pub fn new() -> Self {
        ClosingStmt { sections: Vec::new(), sealed: false, signed: None }
    }

    pub fn write(&mut self, sec: Section) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.sections.len() >= SECTION_ANCHOR {
            return Err(EV_NOT_FIVE);
        }
        if sec.no == 0 || sec.no > SECTION_ANCHOR {
            return Err(EV_RANGE_OUT);
        }
        for e in self.sections.iter() {
            if e.no == sec.no {
                return Err(EV_DUP_SECTION);
            }
        }
        if let Some(last) = self.sections.last() {
            if sec.no != last.no + 1 {
                return Err(EV_OUT_OF_ORDER);
            }
        } else if sec.no != 1 {
            return Err(EV_OUT_OF_ORDER);
        }
        self.sections.push(sec);
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.sections.len()
    }

    pub fn filled_count(&self) -> usize {
        self.sections.iter().filter(|s| s.is_filled()).count()
    }

    /// 封卷：五节齐且逐节体量非空
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.sections.len() != SECTION_ANCHOR {
            self.sealed = true;
            return Err(EV_NOT_FIVE);
        }
        let mut i = 0;
        while i < SECTION_ANCHOR {
            if !self.sections[i].is_filled() {
                self.sealed = true;
                return Err(EV_SECTION_EMPTY);
            }
            i += 1;
        }
        // 卷印：FNV 五节体量拼接
        let mut h: u32 = FNV_SEED;
        let mut i = 0;
        while i < SECTION_ANCHOR {
            let b = self.sections[i].body.as_bytes();
            let mut k = 0;
            while k < b.len() {
                h ^= b[k] as u32;
                h = h.wrapping_mul(0x0100_0193);
                k += 1;
            }
            i += 1;
        }
        self.signed = Some(h);
        self.sealed = true;
        Ok(h)
    }

    pub fn signature_of(&self) -> Option<u32> {
        self.signed
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
}

/// 五节全填构造
fn full_stmt(tag: &str) -> ClosingStmt {
    let mut st = ClosingStmt::new();
    st.write(Section::filled(1, "总览：域四十批收官")).unwrap();
    st.write(Section::filled(2, "判据：800 条全绿")).unwrap();
    st.write(Section::filled(3, "账本：六轴对平")).unwrap();
    st.write(Section::filled(4, "缺陷：闭环率 100")).unwrap();
    let mut s5 = String::from("移交：");
    s5.push_str(tag);
    st.write(Section::filled(5, &s5)).unwrap();
    st
}

pub fn run_unx10394_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10394");
    s.add("J1-段锚", SEG_BASE == 0x1bbc0, "收官陈述段基址 0x1bbc0");
    s.add("J2-节数锚", SECTION_ANCHOR == 5, "五节锚");
    s.add("J3-新册零节", ClosingStmt::new().count() == 0, "新册 0/5");
    s.add("J4-首节必 1", {
        let mut st = ClosingStmt::new();
        st.write(Section::filled(2, "跳节")).unwrap_err() == EV_OUT_OF_ORDER
    }, "首节写 2 EV_OUT_OF_ORDER");
    s.add("J5-节序单调", {
        let mut st = ClosingStmt::new();
        st.write(Section::filled(1, "s1")).unwrap();
        st.write(Section::filled(3, "跳节")).unwrap_err() == EV_OUT_OF_ORDER
    }, "1 后写 3 EV_OUT_OF_ORDER");
    s.add("J6-节号越界", {
        let mut st = ClosingStmt::new();
        st.write(Section::filled(0, "零")).unwrap_err() == EV_RANGE_OUT
            && st.write(Section::filled(6, "溢")).unwrap_err() == EV_RANGE_OUT
    }, "节 0/6 拒收");
    s.add("J7-重复节拒收", {
        let mut st = ClosingStmt::new();
        st.write(Section::filled(1, "s1")).unwrap();
        st.write(Section::filled(1, "again")).unwrap_err() == EV_DUP_SECTION
    }, "重写节 1 EV_DUP_SECTION");
    s.add("J8-满册五节", full_stmt("C4").count() == 5, "五节齐写");
    s.add("J9-满册封卷", {
        let mut st = full_stmt("C4");
        st.seal().is_ok()
    }, "五节齐封卷成功");
    s.add("J10-卷印存档", {
        let mut st = full_stmt("C4");
        let sig = st.seal().unwrap();
        st.signature_of() == Some(sig)
    }, "卷印可回读");
    s.add("J11-卷印确定性", {
        let mut a = full_stmt("C4");
        let mut b = full_stmt("C4");
        a.seal().unwrap() == b.seal().unwrap()
    }, "同文两次封卷同印");
    s.add("J12-文异印异", {
        let mut a = full_stmt("C4");
        let mut b = full_stmt("C5");
        a.seal().unwrap() != b.seal().unwrap()
    }, "尾节 tag 变异印变异");
    s.add("J13-缺节拒封", {
        let mut st = ClosingStmt::new();
        st.write(Section::filled(1, "s1")).unwrap();
        st.seal().unwrap_err() == EV_NOT_FIVE
    }, "一节封卷 EV_NOT_FIVE");
    s.add("J14-空节拒封", {
        let mut st = ClosingStmt::new();
        st.write(Section::filled(1, "短")).unwrap();
        st.write(Section::filled(2, "总览：域四十批收官")).unwrap();
        st.write(Section::filled(3, "判据：800 条全绿")).unwrap();
        st.write(Section::filled(4, "账本：六轴对平")).unwrap();
        st.write(Section::filled(5, "缺陷：闭环率 100")).unwrap();
        st.seal().unwrap_err() == EV_SECTION_EMPTY
    }, "节 1 过短 EV_SECTION_EMPTY");
    s.add("J15-封卷后拒写", {
        let mut st = full_stmt("C4");
        st.seal().unwrap();
        st.write(Section::filled(6, "late")).unwrap_err() == EV_SEALED
    }, "封卷后写节 EV_SEALED");
    s.add("J16-填充计数", {
        let st = full_stmt("C4");
        st.filled_count() == 5
    }, "五节体量全达标 5/5");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_seal_stmt() {
        let mut st = full_stmt("C4");
        let sig = st.seal().unwrap();
        assert_eq!(st.signature_of(), Some(sig));
        assert!(st.is_sealed());
    }

    #[test]
    fn checks_green() {
        let s = run_unx10394_checks();
        assert!(s.all_passed(), "UNX-F10394 判据全绿");
    }
}
