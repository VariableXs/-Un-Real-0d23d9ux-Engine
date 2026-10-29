//! UNX-F10358 对接面 ktest 集成（0x1b740 段）
//!
//! 锚点：UNX-F10358-J1 对接面 ktest 集成——对接用例两侧（A 侧适配器/
//! B 侧适配器）各自跑判各自出真伪判定，判定一次为定不得重跑，任一用
//! 例两侧判定相左即集成不通过，用例数达锚且两侧全跑全判同方算集成通
//! 过。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：用例册（号+名+双侧判定）+ 定音核（一侧一次不重跑）+ 相左核
//! （双侧判定对表）+ 集成核（达锚全跑全同）+ 注入核（重跑/未在册/
//! 重号/相左四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b740 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b740;
pub const IK_ZERO: u32 = SEG_BASE + 0x01; // 用例空名
pub const IK_DUP: u32 = SEG_BASE + 0x02; // 用例重号
pub const IK_NOCASE: u32 = SEG_BASE + 0x03; // 用例未在册
pub const IK_RERUN: u32 = SEG_BASE + 0x04; // 同侧重跑
pub const IK_OPEN: u32 = SEG_BASE + 0x05; // 有用例未全跑
pub const IK_CONFLICT: u32 = SEG_BASE + 0x06; // 判定相左
pub const IK_SEALED: u32 = SEG_BASE + 0x07; // 封账后再动
pub const IK_CAP: u32 = SEG_BASE + 0x08; // 册超容

/// 用例数锚。
pub const CASES_NEEDED: usize = 6;
/// 册定容。
pub const CAP: usize = 10;

/// 对接用例：号 + 名 + A/B 侧判定。
#[derive(Clone)]
pub struct IfaceCase {
    pub cid: u32,
    pub name: String,
    pub verdict_a: Option<bool>,
    pub verdict_b: Option<bool>,
}

impl IfaceCase {
    /// 双侧已判？
    pub fn covered(&self) -> bool {
        self.verdict_a.is_some() && self.verdict_b.is_some()
    }

    /// 双侧判定同？
    pub fn agreed(&self) -> bool {
        match (self.verdict_a, self.verdict_b) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }
}

/// 对接面 ktest 账。
#[derive(Clone)]
pub struct IfaceKtest {
    pub cases: Vec<IfaceCase>,
    pub sealed: bool,
}

impl IfaceKtest {
    /// 空账。
    pub fn new() -> Self {
        IfaceKtest {
            cases: Vec::new(),
            sealed: false,
        }
    }

    /// 找用例。
    pub fn find(&self, cid: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.cases.len() {
            if self.cases[i].cid == cid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登用例。
    pub fn register(&mut self, cid: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(IK_SEALED);
        }
        if name.is_empty() {
            return Err(IK_ZERO);
        }
        if self.cases.len() >= CAP {
            return Err(IK_CAP);
        }
        if self.find(cid).is_some() {
            return Err(IK_DUP);
        }
        self.cases.push(IfaceCase {
            cid,
            name: name.to_string(),
            verdict_a: None,
            verdict_b: None,
        });
        Ok(())
    }

    /// A 侧跑判（一次为定）。
    pub fn run_a(&mut self, cid: u32, ok: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(IK_SEALED);
        }
        let idx = match self.find(cid) {
            Some(i) => i,
            None => return Err(IK_NOCASE),
        };
        if self.cases[idx].verdict_a.is_some() {
            return Err(IK_RERUN);
        }
        self.cases[idx].verdict_a = Some(ok);
        Ok(())
    }

    /// B 侧跑判（一次为定）。
    pub fn run_b(&mut self, cid: u32, ok: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(IK_SEALED);
        }
        let idx = match self.find(cid) {
            Some(i) => i,
            None => return Err(IK_NOCASE),
        };
        if self.cases[idx].verdict_b.is_some() {
            return Err(IK_RERUN);
        }
        self.cases[idx].verdict_b = Some(ok);
        Ok(())
    }

    /// 未全跑用例数。
    pub fn uncovered(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.cases.len() {
            if !self.cases[i].covered() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 判定相左用例号清单。
    pub fn conflicts(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.cases.len() {
            if self.cases[i].covered() && !self.cases[i].agreed() {
                out.push(self.cases[i].cid);
            }
            i += 1;
        }
        out
    }

    /// 双侧同判用例数。
    pub fn agreed_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.cases.len() {
            if self.cases[i].agreed() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 集成通过：达锚且全跑且零相左。
    pub fn integrated(&self) -> bool {
        self.cases.len() >= CASES_NEEDED && self.uncovered() == 0 && self.conflicts().is_empty()
    }

    /// 封账：未集成即拒（未全跑与相左分诊）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(IK_SEALED);
        }
        if self.uncovered() > 0 {
            return Err(IK_OPEN);
        }
        if !self.conflicts().is_empty() {
            return Err(IK_CONFLICT);
        }
        if self.cases.len() < CASES_NEEDED {
            return Err(IK_OPEN);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹：号/双侧判定逐例叠算。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b7401b7;
        let mut i = 0;
        while i < self.cases.len() {
            let c = &self.cases[i];
            h = (h ^ c.cid).wrapping_mul(0x01000193);
            let va = match c.verdict_a {
                Some(true) => 1u32,
                Some(false) => 2,
                None => 0,
            };
            let vb = match c.verdict_b {
                Some(true) => 1u32,
                Some(false) => 2,
                None => 0,
            };
            h = (h ^ (va * 8 + vb)).wrapping_mul(0x01000193);
            i += 1;
        }
        h
    }
}

/// 对接用例样例。
pub fn case_names() -> [(&'static str, u32); 6] {
    [
        ("adp-open", 701),
        ("adp-read", 702),
        ("adp-write", 703),
        ("adp-flush", 704),
        ("adp-close", 705),
        ("adp-err", 706),
    ]
}

/// 注入核：四路坏账必抓。
/// 0: 同侧重跑；1: 未在册用例跑判；2: 用例重号；3: 双侧判定相左封账拒。
pub fn inject_four() -> [bool; 4] {
    let mut t = IfaceKtest::new();
    let mut k = 0;
    while k < case_names().len() {
        let (name, cid) = case_names()[k];
        let _ = t.register(cid, name);
        k += 1;
    }
    let _ = t.run_a(701, true);
    let d0 = t.run_a(701, false).err() == Some(IK_RERUN);
    let d1 = t.run_b(4242, true).err() == Some(IK_NOCASE);
    let d2 = t.register(701, "重号").err() == Some(IK_DUP);
    let d3 = {
        let mut bad = IfaceKtest::new();
        let _ = bad.register(801, "相左样本");
        let _ = bad.run_a(801, true);
        let _ = bad.run_b(801, false);
        bad.seal().err() == Some(IK_CONFLICT)
    };
    [d0, d1, d2, d3]
}

/// 判据：run_unx10358_checks。
pub fn run_unx10358_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10358");
    // J1 正路径：六例入册。
    let mut t = IfaceKtest::new();
    let mut j1 = true;
    let mut k = 0;
    while k < case_names().len() {
        let (name, cid) = case_names()[k];
        if t.register(cid, name).is_err() {
            j1 = false;
        }
        k += 1;
    }
    s.add("J1-六例入册", j1 && t.cases.len() == 6, "对接用例全入册");
    // J2 边界：空名拒。
    let mut t2 = IfaceKtest::new();
    let j2 = t2.register(711, "").err() == Some(IK_ZERO);
    s.add("J2-空名拒", j2, "用例空名即 IK_ZERO");
    // J3 注入：同侧重跑必抓。
    let mut t3 = IfaceKtest::new();
    let _ = t3.register(712, "重跑样本");
    let _ = t3.run_a(712, true);
    let j3 = t3.run_a(712, false).err() == Some(IK_RERUN);
    s.add("J3-重跑抓", j3, "A 侧两跑即 IK_RERUN");
    // J4 注入：未在册跑判必抓。
    let mut t4 = IfaceKtest::new();
    let j4 = t4.run_a(9999, true).err() == Some(IK_NOCASE);
    s.add("J4-无例抓", j4, "未在册跑判即 IK_NOCASE");
    // J5 注入：用例重号必抓。
    let mut t5 = IfaceKtest::new();
    let _ = t5.register(713, "原例");
    let j5 = t5.register(713, "重号").err() == Some(IK_DUP);
    s.add("J5-重号抓", j5, "用例重号即 IK_DUP");
    // J6 边界：未全跑封账拒。
    let mut t6 = IfaceKtest::new();
    let mut k6 = 0;
    while k6 < case_names().len() {
        let (name, cid) = case_names()[k6];
        let _ = t6.register(cid, name);
        let _ = t6.run_a(cid, true);
        if k6 < 3 {
            let _ = t6.run_b(cid, true);
        }
        k6 += 1;
    }
    let j6 = t6.seal().err() == Some(IK_OPEN) && t6.uncovered() == 3;
    s.add("J6-缺跑拒", j6, "三例缺 B 侧即 IK_OPEN");
    // J7 注入：判定相左封账拒。
    let mut t7 = IfaceKtest::new();
    let mut k7 = 0;
    while k7 < case_names().len() {
        let (name, cid) = case_names()[k7];
        let _ = t7.register(cid, name);
        k7 += 1;
    }
    let mut k7b = 0;
    while k7b < case_names().len() {
        let cid = case_names()[k7b].1;
        let _ = t7.run_a(cid, cid != 706);
        let _ = t7.run_b(cid, true);
        k7b += 1;
    }
    let j7 = t7.conflicts() == vec![706];
    s.add("J7-相左录", j7, "706 相左例入冲突清单");
    // J8 正路径：六例双侧全跑全同集成通过。
    let mut t8 = IfaceKtest::new();
    let mut k8 = 0;
    while k8 < case_names().len() {
        let (name, cid) = case_names()[k8];
        let _ = t8.register(cid, name);
        let va = cid != 706;
        let vb = cid != 706;
        let _ = t8.run_a(cid, va);
        let _ = t8.run_b(cid, vb);
        k8 += 1;
    }
    let j8 = t8.integrated() && t8.agreed_count() == 6;
    s.add("J8-集成通", j8, "达锚全跑全同");
    // J9 正路径：集成后封账。
    let mut t9 = t8.clone();
    let j9 = t9.seal().is_ok() && t9.sealed;
    s.add("J9-封账成", j9, "集成通过封账");
    // J10 边界：册数不足锚封账拒。
    let mut t10 = IfaceKtest::new();
    let _ = t10.register(721, "独例");
    let _ = t10.run_a(721, true);
    let _ = t10.run_b(721, true);
    let j10 = t10.seal().err() == Some(IK_OPEN);
    s.add("J10-缺例拒", j10, "一例不足锚即 IK_OPEN");
    // J11 边界：封账后再跑拒。
    let mut t11 = t8.clone();
    let _ = t11.seal();
    let j11 = t11.run_a(701, true).err() == Some(IK_SEALED);
    s.add("J11-封后跑拒", j11, "封账后跑判即 IK_SEALED");
    // J12 动态：相左清单出数。
    let cf = t7.conflicts();
    let j12 = cf == vec![706];
    s.add_dyn(
        "J12-相左出数",
        j12,
        &format!("相左用例号 {:?}", cf),
    );
    // J13 动态：覆盖与指纹。
    let j13 = t8.uncovered() == 0 && t8.fold() != 0;
    s.add_dyn(
        "J13-覆盖出数",
        j13,
        &format!(
            "六例双覆盖同判 {}，指纹 {:08x}",
            t8.agreed_count(),
            t8.fold()
        ),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "重跑/无例/重号/相左四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_seal() {
        let mut t = IfaceKtest::new();
        let mut k = 0;
        while k < case_names().len() {
            let (name, cid) = case_names()[k];
            assert!(t.register(cid, name).is_ok());
            assert!(t.run_a(cid, true).is_ok());
            assert!(t.run_b(cid, true).is_ok());
            k += 1;
        }
        assert!(t.integrated());
        assert!(t.seal().is_ok());
        assert!(t.run_a(701, true).err() == Some(IK_SEALED));
        assert!(t.fold() != 0);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10358_checks();
        assert!(s.all_passed());
    }
}
