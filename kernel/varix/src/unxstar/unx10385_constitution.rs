//! UNX-F10385 域功条件核（宪法五条逐条机检）（0x1baa0 段）
//! 锚点：UNX-F10385-J1 域功条件核五条逐条机检（纯代码/判据/账本/哈希/交接）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1baa0;
pub const EV_NO_PROOF: u32 = SEG_BASE + 1;
pub const EV_NO_CRITERIA: u32 = SEG_BASE + 2;
pub const EV_NO_LEDGER: u32 = SEG_BASE + 3;
pub const EV_NO_HASH: u32 = SEG_BASE + 4;
pub const EV_NO_HANDOFF: u32 = SEG_BASE + 5;
pub const EV_SEALED: u32 = SEG_BASE + 6;

/// 宪法五条编号 1..=5，逐条挂证据链
#[derive(Clone, Debug)]
pub struct Clause {
    pub no: usize,
    pub title: &'static str,
    pub proof: Option<String>,
}

impl Clause {
    pub fn bare(no: usize, title: &'static str) -> Self {
        Clause { no, title, proof: None }
    }
    pub fn proven(no: usize, title: &'static str, proof: &str) -> Self {
        Clause { no, title, proof: Some(proof.to_string()) }
    }
    pub fn has_proof(&self) -> bool {
        self.proof.is_some()
    }
}

/// 域功条件核机检器：五条齐备且逐条有证据方可出 PASS
pub struct Constitution {
    clauses: Vec<Clause>,
    sealed: bool,
    passed: Option<bool>,
}

const FIVE: usize = 5;

impl Constitution {
    pub fn new() -> Self {
        let titles = ["纯功能代码", "判据可跑", "账本对平", "哈希冻结", "交接完备"];
        let mut clauses = Vec::new();
        let mut i = 1;
        while i <= FIVE {
            clauses.push(Clause::bare(i, titles[i - 1]));
            i += 1;
        }
        Constitution { clauses, sealed: false, passed: None }
    }

    pub fn attest(&mut self, no: usize, proof: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if no == 0 || no > FIVE {
            return Err(EV_NO_PROOF);
        }
        if proof.is_empty() {
            return Err(EV_NO_PROOF);
        }
        match no {
            1 => self.clauses[0].proof = Some(proof.to_string()),
            2 => self.clauses[1].proof = Some(proof.to_string()),
            3 => self.clauses[2].proof = Some(proof.to_string()),
            4 => self.clauses[3].proof = Some(proof.to_string()),
            _ => self.clauses[4].proof = Some(proof.to_string()),
        }
        Ok(())
    }

    /// 逐条机检：五条全有证据才 PASS，否则第一条缺失项报对应 EV
    pub fn examine(&mut self) -> Result<bool, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        let mut first_missing: Option<u32> = None;
        let mut i = 0;
        while i < FIVE {
            if !self.clauses[i].has_proof() {
                let ev = match i {
                    0 => EV_NO_PROOF,
                    1 => EV_NO_CRITERIA,
                    2 => EV_NO_LEDGER,
                    3 => EV_NO_HASH,
                    _ => EV_NO_HANDOFF,
                };
                if first_missing.is_none() {
                    first_missing = Some(ev);
                }
            }
            i += 1;
        }
        if let Some(ev) = first_missing {
            self.sealed = true;
            self.passed = Some(false);
            return Err(ev);
        }
        self.sealed = true;
        self.passed = Some(true);
        Ok(true)
    }

    pub fn clause_titles(&self) -> Vec<String> {
        let mut v = Vec::new();
        for c in self.clauses.iter() {
            v.push(c.title.to_string());
        }
        v
    }

    pub fn proved_count(&self) -> usize {
        self.clauses.iter().filter(|c| c.has_proof()).count()
    }

    pub fn missing_nos(&self) -> Vec<usize> {
        let mut v = Vec::new();
        for c in self.clauses.iter() {
            if !c.has_proof() {
                v.push(c.no);
            }
        }
        v
    }

    pub fn result_of(&self) -> Option<bool> {
        self.passed
    }
}

/// 五条注入样例：全证 PASS / 缺判据 / 缺哈希
pub fn inject_three() -> [(&'static str, Result<(), u32>); 3] {
    let mut full = Constitution::new();
    full.attest(1, "code-only").unwrap();
    full.attest(2, "criteria-runnable").unwrap();
    full.attest(3, "ledger-balanced").unwrap();
    full.attest(4, "hash-frozen").unwrap();
    full.attest(5, "handoff-complete").unwrap();
    let r1 = full.examine().map(|_| ());

    let mut nocrit = Constitution::new();
    nocrit.attest(1, "code").unwrap();
    nocrit.attest(3, "ledger").unwrap();
    nocrit.attest(4, "hash").unwrap();
    nocrit.attest(5, "handoff").unwrap();
    let r2 = nocrit.examine().map(|_| ());

    let mut nohash = Constitution::new();
    nohash.attest(1, "code").unwrap();
    nohash.attest(2, "crit").unwrap();
    nohash.attest(3, "ledger").unwrap();
    nohash.attest(5, "handoff").unwrap();
    let r3 = nohash.examine().map(|_| ());

    [("full_pass", r1), ("miss_criteria", r2), ("miss_hash", r3)]
}

pub fn run_unx10385_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10385");
    s.add("J1-段锚", SEG_BASE == 0x1baa0, "宪法五条段基址 0x1baa0");
    s.add("J2-新册五条", Constitution::new().clause_titles().len() == FIVE, "新册固定五条");
    s.add("J3-新册零证", Constitution::new().proved_count() == 0, "新册零证据");
    s.add("J4-新册缺号表", Constitution::new().missing_nos() == vec![1, 2, 3, 4, 5], "未证缺号 1..=5");
    s.add("J5-空号拒证", Constitution::new().attest(0, "x").unwrap_err() == EV_NO_PROOF, "编号 0 越界");
    s.add("J6-溢号拒证", Constitution::new().attest(6, "x").unwrap_err() == EV_NO_PROOF, "编号 6 越界");
    s.add("J7-空证拒收", Constitution::new().attest(1, "").unwrap_err() == EV_NO_PROOF, "空证据串拒收");
    s.add("J8-缺判据报码", inject_three()[1].1 == Err(EV_NO_CRITERIA), "注入样例二缺第 2 条");
    s.add("J9-缺哈希报码", inject_three()[2].1 == Err(EV_NO_HASH), "注入样例三缺第 4 条");
    s.add("J10-五证过检", inject_three()[0].1.is_ok(), "注入样例一五证 PASS");
    s.add("J11-逐条挂证", {
        let mut c = Constitution::new();
        let mut ok = true;
        let proofs = ["a", "b", "c", "d", "e"];
        let mut n = 1;
        while n <= FIVE {
            ok = ok && c.attest(n, proofs[n - 1]).is_ok();
            n += 1;
        }
        ok && c.proved_count() == FIVE
    }, "五条逐条挂证成功");
    s.add("J12-检后封账", {
        let mut c = Constitution::new();
        c.attest(1, "x").unwrap();
        c.attest(2, "x").unwrap();
        c.attest(3, "x").unwrap();
        c.attest(4, "x").unwrap();
        c.attest(5, "x").unwrap();
        c.examine().unwrap();
        c.examine().unwrap_err() == EV_SEALED
    }, "机检一次即封账");
    s.add("J13-过检结果存档", {
        let mut c = Constitution::new();
        let proofs = ["p1", "p2", "p3", "p4", "p5"];
        let mut n = 1;
        while n <= FIVE {
            c.attest(n, proofs[n - 1]).unwrap();
            n += 1;
        }
        c.examine().unwrap();
        c.result_of() == Some(true)
    }, "PASS 结果可回读");
    s.add("J14-败检结果存档", {
        let mut c = Constitution::new();
        let _ = c.examine();
        c.result_of() == Some(false)
    }, "FAIL 结果可回读");
    s.add("J15-证后封账拒改", {
        let mut c = Constitution::new();
        c.attest(1, "x").unwrap();
        c.examine().unwrap_err() == EV_NO_CRITERIA;
        c.attest(2, "late").unwrap_err() == EV_SEALED
    }, "机检后追加证据 EV_SEALED");
    s.add_dyn("J16-缺项首报次序", {
        let mut c = Constitution::new();
        c.attest(5, "only5").unwrap();
        let e1 = c.examine().unwrap_err();
        e1 == EV_NO_PROOF && c.missing_nos().len() == 4
    }, "缺 1..=4 时首报第 1 条且缺号表 4 项");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_five_clauses() {
        let inj = inject_three();
        assert!(inj[0].1.is_ok());
        assert_eq!(inj[1].1, Err(EV_NO_CRITERIA));
        assert_eq!(inj[2].1, Err(EV_NO_HASH));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10385_checks();
        assert!(s.all_passed(), "UNX-F10385 判据全绿");
    }
}
