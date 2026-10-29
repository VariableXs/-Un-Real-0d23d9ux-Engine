//! UNX-F10388 判据三成分终核（800 条判据复测/拦截语义核）（0x1bb00 段）
//! 锚点：UNX-F10388-J1 判据三成分终核（复测可跑/拦截语义/双成分齐备）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bb00;
pub const EV_NO_NAME: u32 = SEG_BASE + 1;
pub const EV_NO_SEMANTIC: u32 = SEG_BASE + 2;
pub const EV_NO_RERUN: u32 = SEG_BASE + 3;
pub const EV_NOT_EIGHT: u32 = SEG_BASE + 4;
pub const EV_SEALED: u32 = SEG_BASE + 5;
pub const EV_DUP_NAME: u32 = SEG_BASE + 6;

pub const CRITERIA_ANCHOR: usize = 800;

/// 判据三成分：名（可索引）/ 拦截语义（红时挡闸）/ 复测面（可重跑）
#[derive(Clone, Debug)]
pub struct Criterion {
    pub name: String,
    pub semantic: &'static str,
    pub rerunnable: bool,
    pub rerun_hits: usize,
}

impl Criterion {
    pub fn new(name: &str, semantic: &'static str, rerunnable: bool) -> Self {
        Criterion { name: name.to_string(), semantic, rerunnable, rerun_hits: 0 }
    }
    pub fn complete(&self) -> bool {
        !self.name.is_empty() && !self.semantic.is_empty() && self.rerunnable
    }
}

/// 三成分终核册
pub struct TripleAudit {
    items: Vec<Criterion>,
    sealed: bool,
    intercept_armed: bool,
}

const CAP: usize = 800;

impl TripleAudit {
    pub fn new() -> Self {
        TripleAudit { items: Vec::new(), sealed: false, intercept_armed: false }
    }

    pub fn enroll(&mut self, c: Criterion) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.items.len() >= CAP {
            return Err(EV_NOT_EIGHT);
        }
        if c.name.is_empty() {
            return Err(EV_NO_NAME);
        }
        if c.semantic.is_empty() {
            return Err(EV_NO_SEMANTIC);
        }
        if !c.rerunnable {
            return Err(EV_NO_RERUN);
        }
        for e in self.items.iter() {
            if e.name == c.name {
                return Err(EV_DUP_NAME);
            }
        }
        self.items.push(c);
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// 复测语义：每条判据可重复跑，命中次数单调递增
    pub fn rerun(&mut self, name: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].name == name {
                self.items[i].rerun_hits += 1;
                return Ok(self.items[i].rerun_hits);
            }
            i += 1;
        }
        Err(EV_NO_NAME)
    }

    /// 拦截语义闸：任一判据 incomplete 时 armed 闸关（拦截）
    pub fn arm_intercept(&mut self) -> bool {
        let mut all_ok = true;
        let mut i = 0;
        while i < self.items.len() {
            if !self.items[i].complete() {
                all_ok = false;
            }
            i += 1;
        }
        self.intercept_armed = all_ok && !self.items.is_empty();
        self.intercept_armed
    }

    pub fn intercept_state(&self) -> bool {
        self.intercept_armed
    }

    /// 终核：800 条且拦截闸开
    pub fn finalize(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.items.len() != CAP {
            self.sealed = true;
            return Err(EV_NOT_EIGHT);
        }
        if !self.arm_intercept() {
            self.sealed = true;
            return Err(EV_NO_SEMANTIC);
        }
        let n = self.items.len();
        self.sealed = true;
        Ok(n)
    }

    pub fn rerun_total(&self) -> usize {
        let mut acc = 0;
        let mut i = 0;
        while i < self.items.len() {
            acc += self.items[i].rerun_hits;
            i += 1;
        }
        acc
    }
}

/// 小样册构造（30 条，快速验证语义）
fn mini_book() -> TripleAudit {
    let mut t = TripleAudit::new();
    let mut n = 1;
    while n <= 30 {
        let name = if n < 10 {
            let mut s = String::from("c00");
            s.push((48 + n as u8) as char);
            s
        } else {
            let mut s = String::from("c0");
            s.push((48 + n / 10) as u8 as char);
            s.push((48 + n % 10) as u8 as char);
            s
        };
        t.enroll(Criterion::new(&name, "拦截红项", true)).unwrap();
        n += 1;
    }
    t
}

pub fn run_unx10388_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10388");
    s.add("J1-段锚", SEG_BASE == 0x1bb00, "三成分终核段基址 0x1bb00");
    s.add("J2-总量锚", CRITERIA_ANCHOR == 800, "判据总量锚 800");
    s.add("J3-小样册齐备", {
        let t = mini_book();
        t.count() == 30
    }, "30 条小样册入册");
    s.add("J4-空名拒收", TripleAudit::new().enroll(Criterion::new("", "x", true)).unwrap_err() == EV_NO_NAME, "空名 EV_NO_NAME");
    s.add("J5-空语义拒收", TripleAudit::new().enroll(Criterion::new("n", "", true)).unwrap_err() == EV_NO_SEMANTIC, "空语义 EV_NO_SEMANTIC");
    s.add("J6-不可复测拒收", TripleAudit::new().enroll(Criterion::new("n", "x", false)).unwrap_err() == EV_NO_RERUN, "不可复测 EV_NO_RERUN");
    s.add("J7-重名拒收", {
        let mut t = TripleAudit::new();
        t.enroll(Criterion::new("dup", "x", true)).unwrap();
        t.enroll(Criterion::new("dup", "y", true)).unwrap_err() == EV_DUP_NAME
    }, "重名 EV_DUP_NAME");
    s.add("J8-复测单调", {
        let mut t = mini_book();
        t.rerun("c001") == Ok(1) && t.rerun("c001") == Ok(2) && t.rerun("c001") == Ok(3)
    }, "同名复测 1→2→3");
    s.add("J9-复测总数", {
        let mut t = mini_book();
        t.rerun("c001").unwrap();
        t.rerun("c002").unwrap();
        t.rerun("c002").unwrap();
        t.rerun_total() == 3
    }, "复测命中总数 3");
    s.add("J10-复测未知名", {
        let mut t = mini_book();
        t.rerun("zzz") == Err(EV_NO_NAME)
    }, "未知名 EV_NO_NAME");
    s.add("J11-拦截闸开", {
        let mut t = mini_book();
        t.arm_intercept()
    }, "全齐备拦截闸开");
    s.add("J12-缺额拦截闸", {
        let mut t = TripleAudit::new();
        t.enroll(Criterion::new("only", "x", true)).unwrap();
        !t.arm_intercept()
    }, "1 条不足拦截闸仍关（终核面）");
    s.add("J13-终验缺额", {
        let mut t = mini_book();
        t.finalize().unwrap_err() == EV_NOT_EIGHT
    }, "30 条终验 EV_NOT_EIGHT");
    s.add("J14-终验封账", {
        let mut t = mini_book();
        t.finalize().unwrap_err() == EV_NOT_EIGHT;
        t.enroll(Criterion::new("late", "x", true)).unwrap_err() == EV_SEALED
    }, "终验后入册 EV_SEALED");
    s.add("J15-成分齐备判定", {
        let c1 = Criterion::new("a", "b", true);
        let c2 = Criterion::new("a", "b", false);
        c1.complete() && !c2.complete()
    }, "三成分齐备逐条可判");
    s.add_dyn("J16-复测复检双通过", {
        let mut t = mini_book();
        let h1 = t.rerun("c030").unwrap();
        let h2 = t.rerun("c030").unwrap();
        h1 == 1 && h2 == 2
    }, "尾条 c030 复测 1-2 两轮");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_mini_semantics() {
        let mut t = mini_book();
        assert_eq!(t.rerun("c015").unwrap(), 1);
        assert!(t.arm_intercept());
        assert_eq!(t.finalize().unwrap_err(), EV_NOT_EIGHT);
    }

    #[test]
    fn checks_green() {
        let s = run_unx10388_checks();
        assert!(s.all_passed(), "UNX-F10388 判据全绿");
    }
}
