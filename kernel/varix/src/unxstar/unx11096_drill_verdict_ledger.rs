//! UNX-F11096 演习结果三态总账（绿/异常/待办汇总）（0x1ec00 段）
//!
//! 锚点：UNX-F11096-J1 三态账逐族对平——绿/异常/待办三态逐族入账对平；
//! J2 总账守恒闭合——三态计数之和等于演习件总数、分族之和等于总账。
//! 域：CoRun·UNX-C4（长稳监控域）。
//!
//! 结构：三态账（每件：族 + 态，态机 Green/Abnormal/Todo 单向流转）+
//! 汇总器（分族分态计数对账）+ 注入核（重件/未知态流转/未知族/封账写入
//! 四路必抓）。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x1ec00 独占 0x20）。
pub const SEG_BASE: u32 = 0x1ec00;
pub const EX_DUP: u32 = SEG_BASE + 0x01; // 演习件重号
pub const EX_NOENT: u32 = SEG_BASE + 0x02; // 件不在册
pub const EX_FLOW: u32 = SEG_BASE + 0x03; // 非法态流转
pub const EX_FROZEN: u32 = SEG_BASE + 0x04; // 封账后写账
pub const EX_FAM: u32 = SEG_BASE + 0x05; // 未知族

/// 三态。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Verdict {
    Green,
    Abnormal,
    Todo,
}

/// 演习族。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fam {
    Drill,
    Soak,
    Chaos,
}

/// 演习件。
#[derive(Clone, Copy, Debug)]
pub struct CaseItem {
    pub id: u32,
    pub fam: Fam,
    pub verdict: Verdict,
}

/// 三态总账。
pub struct ExBook {
    pub cases: Vec<CaseItem>,
    pub sealed: bool,
}

impl ExBook {
    pub fn new() -> ExBook {
        ExBook { cases: Vec::new(), sealed: false }
    }

    fn at(&self, id: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].id == id {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记演习件（初始 Green）。
    pub fn enroll(&mut self, id: u32, fam: Fam) -> Result<(), u32> {
        if self.sealed {
            return Err(EX_FROZEN);
        }
        if self.at(id).is_some() {
            return Err(EX_DUP);
        }
        self.cases.push(CaseItem { id, fam, verdict: Verdict::Green });
        Ok(())
    }

    /// 态流转：Green→Abnormal/Todo 单向，Abnormal→Todo 允许（修复转待办），
    /// 其余回退必拒。
    pub fn mark(&mut self, id: u32, v: Verdict) -> Result<(), u32> {
        if self.sealed {
            return Err(EX_FROZEN);
        }
        match self.at(id) {
            None => Err(EX_NOENT),
            Some(i) => {
                let cur = self.cases[i].verdict;
                let ok = match (cur, v) {
                    (Verdict::Green, Verdict::Abnormal) => true,
                    (Verdict::Green, Verdict::Todo) => true,
                    (Verdict::Abnormal, Verdict::Todo) => true,
                    _ => false,
                };
                if !ok {
                    return Err(EX_FLOW);
                }
                self.cases[i].verdict = v;
                Ok(())
            }
        }
    }

    /// 分态计数。
    pub fn count(&self, v: Verdict) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].verdict == v {
                n = n.wrapping_add(1);
            }
            k += 1;
        }
        n
    }

    /// 分族分态计数。
    pub fn count_fam(&self, f: Fam, v: Verdict) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].fam == f && self.cases[k].verdict == v {
                n = n.wrapping_add(1);
            }
            k += 1;
        }
        n
    }

    /// 总账守恒：三态和 = 件总数；分族三态和 = 全族计数。
    pub fn conserved(&self) -> bool {
        let total = self.cases.len() as u32;
        let sum = self.count(Verdict::Green)
            .wrapping_add(self.count(Verdict::Abnormal))
            .wrapping_add(self.count(Verdict::Todo));
        if sum != total {
            return false;
        }
        let fams = [Fam::Drill, Fam::Soak, Fam::Chaos];
        let mut k = 0usize;
        while k < fams.len() {
            let fsum = self.count_fam(fams[k], Verdict::Green)
                .wrapping_add(self.count_fam(fams[k], Verdict::Abnormal))
                .wrapping_add(self.count_fam(fams[k], Verdict::Todo));
            let mut ftotal = 0u32;
            let mut j = 0usize;
            while j < self.cases.len() {
                if self.cases[j].fam == fams[k] {
                    ftotal = ftotal.wrapping_add(1);
                }
                j += 1;
            }
            if fsum != ftotal {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 全绿裁断。
    pub fn all_green(&self) -> bool {
        self.count(Verdict::Green) == self.cases.len() as u32 && !self.cases.is_empty()
    }

    /// fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0x1ec0_02a6;
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = (h ^ c.id as u64).wrapping_mul(0x100000001b3);
            h = (h ^ match c.verdict { Verdict::Green => 1u64, Verdict::Abnormal => 2, Verdict::Todo => 3 })
                .wrapping_mul(0x100000001b3);
            h = (h ^ match c.fam { Fam::Drill => 1u64, Fam::Soak => 2, Fam::Chaos => 3 })
                .wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 件重号必抓
    let mut b = ExBook::new();
    let _ = b.enroll(1, Fam::Drill);
    got[0] = b.enroll(1, Fam::Soak).err() == Some(EX_DUP);
    // 2) 未知件与非法流转必抓
    let mut c = ExBook::new();
    got[1] = c.mark(9, Verdict::Todo).err() == Some(EX_NOENT);
    let _ = c.enroll(2, Fam::Drill);
    let _ = c.mark(2, Verdict::Abnormal);
    got[1] = got[1] && c.mark(2, Verdict::Green).err() == Some(EX_FLOW);
    // 3) 回退流转必抓（Todo→Green）
    let _ = c.mark(2, Verdict::Todo);
    got[2] = c.mark(2, Verdict::Green).err() == Some(EX_FLOW)
        && c.mark(2, Verdict::Abnormal).err() == Some(EX_FLOW);
    // 4) 封账写入必抓
    c.seal();
    got[3] = c.enroll(3, Fam::Drill).err() == Some(EX_FROZEN)
        && c.mark(2, Verdict::Todo).err() == Some(EX_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11096_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11096");

    // J1 三态账逐族对平
    let mut b = ExBook::new();
    let mut id = 1u32;
    let fams = [Fam::Drill, Fam::Soak, Fam::Chaos];
    let mut k = 0usize;
    while k < 30 {
        let _ = b.enroll(id, fams[k % 3]);
        id = id.wrapping_add(1);
        k += 1;
    }
    // 6 件转异常，3 件转待办。
    let mut t = 0u32;
    while t < 6 {
        let _ = b.mark(t.wrapping_add(1), Verdict::Abnormal);
        t = t.wrapping_add(1);
    }
    let mut t = 0u32;
    while t < 3 {
        let _ = b.mark(t.wrapping_add(7), Verdict::Todo);
        t = t.wrapping_add(1);
    }
    let inj = inject_four();
    let j1 = b.count(Verdict::Green) == 21 && b.count(Verdict::Abnormal) == 6
        && b.count(Verdict::Todo) == 3 && b.count_fam(Fam::Drill, Verdict::Abnormal) == 2
        && inj[0];
    s.add("J1-三态逐族对平", j1, "绿 21/异常 6/待办 3 分族计数逐一对平");

    // J2 总账守恒闭合
    let j2 = b.conserved() && b.cases.len() == 30;
    s.add("J2-总账守恒", j2, "三态和等于件总数且分族守恒");

    // J3 重号与未知件必抓
    s.add("J3-重号未知抓", inj[0] && inj[1], "件重号与未知件标记必拒");

    // J4 非法流转必抓
    s.add("J4-流转抓", inj[1], "回退流转必拒");

    // J5 终态锁定必抓
    s.add("J5-终态抓", inj[2], "Todo 终态不可回退两路必拒");

    // J6 封账写入必抓
    s.add("J6-封账拒写", inj[3], "封账后登记与标记两路均必拒");

    // J7 全绿裁断：有异常即假
    let j7 = !b.all_green() && b.count(Verdict::Green) == 21;
    s.add("J7-全绿裁断", j7, "非全绿账 all_green 为假");

    // J8 全绿账恒真
    let mut c = ExBook::new();
    let mut id = 1u32;
    while id <= 10 {
        let _ = c.enroll(id, Fam::Soak);
        id = id.wrapping_add(1);
    }
    let j8 = c.all_green() && c.conserved();
    s.add("J8-全绿恒真", j8, "全绿账裁断为真且守恒");

    // J9 态机合法路径：Green→Abnormal→Todo 全通过
    let mut d = ExBook::new();
    let _ = d.enroll(1, Fam::Chaos);
    let p1 = d.mark(1, Verdict::Abnormal);
    let p2 = d.mark(1, Verdict::Todo);
    let j9 = p1.is_ok() && p2.is_ok() && d.count(Verdict::Todo) == 1;
    s.add("J9-合法路径", j9, "修复转待办合法路径全通");

    // J10 fold 指纹与封账根
    let fp0 = b.fold();
    let root = b.seal();
    let j10 = fp0 == root && b.fold() == root;
    s.add("J10-指纹冻结", j10, "封账根与 fold 一致且二次 fold 稳定");

    // J11 重建幂等：同流转重放指纹一致
    let mut e = ExBook::new();
    let mut id = 1u32;
    while id <= 30 {
        let _ = e.enroll(id, fams[(id - 1) as usize % 3]);
        id = id.wrapping_add(1);
    }
    let mut t = 0u32;
    while t < 6 {
        let _ = e.mark(t.wrapping_add(1), Verdict::Abnormal);
        t = t.wrapping_add(1);
    }
    let mut t = 0u32;
    while t < 3 {
        let _ = e.mark(t.wrapping_add(7), Verdict::Todo);
        t = t.wrapping_add(1);
    }
    let j11 = e.fold() == fp0;
    s.add("J11-重建幂等", j11, "同流转重放总账指纹一致");

    // J12 三态计数明细落账
    s.add_dyn("J12-三态明细", b.count(Verdict::Green) == 21,
        &format!("g={} a={} t={}", b.count(Verdict::Green),
            b.count(Verdict::Abnormal), b.count(Verdict::Todo)));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11096_verdict_flow() {
        let mut b = ExBook::new();
        assert!(b.enroll(1, Fam::Drill).is_ok());
        assert!(b.enroll(1, Fam::Soak).err() == Some(EX_DUP));
        assert!(b.mark(9, Verdict::Todo).err() == Some(EX_NOENT));
        assert!(b.mark(1, Verdict::Abnormal).is_ok());
        assert!(b.mark(1, Verdict::Green).err() == Some(EX_FLOW));
        assert!(b.mark(1, Verdict::Todo).is_ok());
        assert!(b.mark(1, Verdict::Abnormal).err() == Some(EX_FLOW));
        assert!(b.conserved());
        b.seal();
        assert!(b.enroll(2, Fam::Drill).err() == Some(EX_FROZEN));
    }

    #[test]
    fn unx11096_checks_green() {
        let s = run_unx11096_checks();
        assert!(s.all_passed(), "UNX-F11096 判据未全绿");
    }
}
