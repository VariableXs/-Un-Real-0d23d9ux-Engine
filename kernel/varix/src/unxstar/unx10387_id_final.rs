//! UNX-F10387 800 ID 终核（表头级唯一+两册同构终验）（0x1bae0 段）
//! 锚点：UNX-F10387-J1 800 ID 终核（表头唯一+批册/深化册同构）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bae0;
pub const EV_DUP_ID: u32 = SEG_BASE + 1;
pub const EV_RANGE_OUT: u32 = SEG_BASE + 2;
pub const EV_NOT_EIGHT: u32 = SEG_BASE + 3;
pub const EV_STRUCTURE_DRIFT: u32 = SEG_BASE + 4;
pub const EV_SEALED: u32 = SEG_BASE + 5;
pub const EV_EMPTY: u32 = SEG_BASE + 6;

pub const ID_ANCHOR: usize = 800;

/// 两册同构：批册与深化册对同一 800 ID 各持一份结构
pub struct IdLedger {
    name: &'static str,
    ids: Vec<u16>,
    sealed: bool,
}

impl IdLedger {
    pub fn new(name: &'static str) -> Self {
        IdLedger { name, ids: Vec::new(), sealed: false }
    }

    pub fn label(&self) -> &'static str {
        self.name
    }

    pub fn add(&mut self, id: u16) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if id < 10100 || id >= 10900 {
            return Err(EV_RANGE_OUT);
        }
        for e in self.ids.iter() {
            if *e == id {
                return Err(EV_DUP_ID);
            }
        }
        self.ids.push(id);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// 表头级唯一性：排序后相邻不等
    pub fn unique_sorted(&self) -> bool {
        let mut v: Vec<u16> = Vec::new();
        for e in self.ids.iter() {
            v.push(*e);
        }
        // 简单插入排序（小集合，800 内可承受）
        let mut i = 1;
        while i < v.len() {
            let key = v[i];
            let mut j = i;
            while j > 0 && v[j - 1] > key {
                v[j] = v[j - 1];
                j -= 1;
            }
            v[j] = key;
            i += 1;
        }
        let mut i = 1;
        while i < v.len() {
            if v[i - 1] == v[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 终验：本册 800 项且唯一
    pub fn finalize(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.ids.is_empty() {
            self.sealed = true;
            return Err(EV_EMPTY);
        }
        if self.ids.len() != ID_ANCHOR {
            self.sealed = true;
            return Err(EV_NOT_EIGHT);
        }
        if !self.unique_sorted() {
            self.sealed = true;
            return Err(EV_DUP_ID);
        }
        let n = self.ids.len();
        self.sealed = true;
        Ok(n)
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 两册同构终验：同长 + 逐位相同（集合语义）
    pub fn isomorphic(a: &IdLedger, b: &IdLedger) -> Result<(), u32> {
        if a.sealed || b.sealed {
            return Err(EV_SEALED);
        }
        if a.len() != b.len() {
            return Err(EV_STRUCTURE_DRIFT);
        }
        let mut i = 0;
        while i < a.len() {
            if a.ids[i] != b.ids[i] {
                return Err(EV_STRUCTURE_DRIFT);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn first_of(&self) -> Option<u16> {
        self.ids.first().copied()
    }

    pub fn last_of(&self) -> Option<u16> {
        self.ids.last().copied()
    }
}

/// 构造一段连续 ID 序列（start..=end）
fn fill_range(l: &mut IdLedger, start: u16, end: u16) {
    let mut id = start;
    while id <= end {
        l.add(id).unwrap();
        id += 1;
    }
}

pub fn run_unx10387_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10387");
    s.add("J1-段锚", SEG_BASE == 0x1bae0, "ID 终核段基址 0x1bae0");
    s.add("J2-总量锚", ID_ANCHOR == 800, "ID 总量锚 800");
    s.add("J3-区间锚", {
        let mut l = IdLedger::new("probe");
        l.add(10099).unwrap_err() == EV_RANGE_OUT && l.add(10900).unwrap_err() == EV_RANGE_OUT
    }, "10100 之下/10900 之上拒收");
    s.add("J4-区间收端可入", {
        let mut l = IdLedger::new("probe");
        l.add(10100).is_ok() && l.add(10899).is_ok()
    }, "10100 与 10899 可入册");
    s.add("J5-重号拒收", {
        let mut l = IdLedger::new("probe");
        l.add(10101).unwrap();
        l.add(10101).unwrap_err() == EV_DUP_ID
    }, "同 ID 二次入册 EV_DUP_ID");
    s.add("J6-唯一排序核", {
        let mut l = IdLedger::new("probe");
        fill_range(&mut l, 10100, 10120);
        l.unique_sorted()
    }, "连续序列唯一排序真");
    s.add("J7-空册终验", {
        let mut l = IdLedger::new("probe");
        l.finalize().unwrap_err() == EV_EMPTY
    }, "空册终验 EV_EMPTY");
    s.add("J8-缺额终验", {
        let mut l = IdLedger::new("probe");
        fill_range(&mut l, 10100, 10199);
        l.finalize().unwrap_err() == EV_NOT_EIGHT
    }, "100 项 EV_NOT_EIGHT");
    s.add("J9-同构对真", {
        let mut a = IdLedger::new("batch");
        let mut b = IdLedger::new("deep");
        fill_range(&mut a, 10300, 10309);
        fill_range(&mut b, 10300, 10309);
        IdLedger::isomorphic(&a, &b).is_ok()
    }, "两册同序同长同构");
    s.add("J10-同构对位漂", {
        let mut a = IdLedger::new("batch");
        let mut b = IdLedger::new("deep");
        fill_range(&mut a, 10300, 10309);
        fill_range(&mut b, 10300, 10308);
        b.add(10311).unwrap();
        IdLedger::isomorphic(&a, &b) == Err(EV_STRUCTURE_DRIFT)
    }, "尾位漂移 EV_STRUCTURE_DRIFT");
    s.add("J11-同构长漂", {
        let mut a = IdLedger::new("batch");
        let mut b = IdLedger::new("deep");
        fill_range(&mut a, 10300, 10309);
        fill_range(&mut b, 10300, 10308);
        IdLedger::isomorphic(&a, &b) == Err(EV_STRUCTURE_DRIFT)
    }, "长度不同 EV_STRUCTURE_DRIFT");
    s.add("J12-首尾可读", {
        let mut l = IdLedger::new("probe");
        fill_range(&mut l, 10200, 10209);
        l.first_of() == Some(10200) && l.last_of() == Some(10209)
    }, "首 10200 尾 10209");
    s.add("J13-终验封账", {
        let mut l = IdLedger::new("probe");
        fill_range(&mut l, 10100, 10179);
        l.finalize().unwrap_err() == EV_NOT_EIGHT;
        l.add(10181).unwrap_err() == EV_SEALED
    }, "终验后入册 EV_SEALED");
    s.add("J14-终验通过计值", {
        let mut l = IdLedger::new("probe");
        fill_range(&mut l, 10100, 10899);
        l.finalize() == Ok(800)
    }, "全 800 段终验 Ok(800)");
    s.add("J15-终验唯一防线", {
        let mut l = IdLedger::new("probe");
        let mut n = 0;
        while n < 400 {
            l.add(10100).unwrap_or(()); // 重复会拒，改走分段
            n += 1;
        }
        // 上循环只入一个 10100（其余 EV_DUP_ID 忽略），补满其余段
        let mut id = 10101;
        while id <= 10499 && l.len() < 400 {
            l.add(id).unwrap();
            id += 1;
        }
        while id <= 10899 {
            l.add(id).unwrap();
            id += 1;
        }
        l.finalize() == Ok(800)
    }, "混合路径仍 800 唯一通过");
    s.add_dyn("J16-两册终态拒同构", {
        let mut a = IdLedger::new("batch");
        let mut b = IdLedger::new("deep");
        fill_range(&mut a, 10300, 10319);
        fill_range(&mut b, 10300, 10319);
        a.finalize().unwrap_err() == EV_NOT_EIGHT;
        IdLedger::isomorphic(&a, &b).is_err()
    }, "任一册已封 EV_SEALED 优先");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_full_eight_hundred() {
        let mut a = IdLedger::new("batch");
        let mut b = IdLedger::new("deep");
        fill_range(&mut a, 10100, 10899);
        fill_range(&mut b, 10100, 10899);
        assert_eq!(a.finalize().unwrap(), 800);
        assert_eq!(b.finalize().unwrap(), 800);
    }

    #[test]
    fn checks_green() {
        let s = run_unx10387_checks();
        assert!(s.all_passed(), "UNX-F10387 判据全绿");
    }
}
