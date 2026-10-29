//! UNX-F10307 联签样例跑批（跨域样例联合执行）（0x1b0e0 段）
//!
//! 锚点：UNX-F10307-J1 联合执行——每条样例甲乙双侧同跑且结论与预期
//! 三方一致，半跑/异裁/未知样例即跑批失败。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：跑批册（样例登记，预期值预置）+ 双侧执行核（甲/乙输出位独立
//! 落账，重跑即拒）+ 联合裁断（双侧同跑+同裁+合预期三重）+ 注入核
//! （半跑/异裁/未知样例/重跑四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b0e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b0e0;
pub const JB_DUP: u32 = SEG_BASE + 0x01; // 样例号重号
pub const JB_EMPTY: u32 = SEG_BASE + 0x02; // 域标空
pub const JB_CAP: u32 = SEG_BASE + 0x03; // 册超容
pub const JB_NOCASE: u32 = SEG_BASE + 0x04; // 未知样例
pub const JB_RERUN: u32 = SEG_BASE + 0x05; // 已跑再跑
pub const JB_HALF: u32 = SEG_BASE + 0x06; // 半跑不可裁
pub const JB_SEALED: u32 = SEG_BASE + 0x07; // 封批后提交

/// 跑批册定容。
pub const CAP: usize = 16;
/// 样例数锚。
pub const CASES_NEEDED: usize = 8;

/// 单条联签样例。
#[derive(Clone)]
pub struct BatchCase {
    pub case_id: u32,
    pub domain_tag: String,
    pub expect: bool,
    pub a_out: Option<bool>,
    pub b_out: Option<bool>,
}

impl BatchCase {
    /// 双侧均已跑。
    pub fn both_ran(&self) -> bool {
        self.a_out.is_some() && self.b_out.is_some()
    }

    /// 双侧异裁。
    pub fn conflict(&self) -> bool {
        match (self.a_out, self.b_out) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        }
    }

    /// 联合通过：双侧同跑、同裁、合预期。
    pub fn joint_pass(&self) -> bool {
        self.both_ran() && !self.conflict() && self.a_out == Some(self.expect)
    }
}

/// 联签样例跑批册。
pub struct JointRunner {
    pub cases: Vec<BatchCase>,
    pub sealed: bool,
}

impl JointRunner {
    pub fn new() -> JointRunner {
        JointRunner { cases: Vec::new(), sealed: false }
    }

    fn pos(&self, id: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].case_id == id {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 提交样例：封批、重号、空域标、超容均拒。
    pub fn submit(&mut self, id: u32, tag: &str, expect: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(JB_SEALED);
        }
        if id == 0 || self.pos(id).is_some() {
            return Err(JB_DUP);
        }
        if tag.is_empty() {
            return Err(JB_EMPTY);
        }
        if self.cases.len() >= CAP {
            return Err(JB_CAP);
        }
        self.cases.push(BatchCase {
            case_id: id,
            domain_tag: tag.to_string(),
            expect,
            a_out: None,
            b_out: None,
        });
        Ok(())
    }

    /// 甲侧执行落账。
    pub fn a_run(&mut self, id: u32, ok: bool) -> Result<(), u32> {
        match self.pos(id) {
            None => Err(JB_NOCASE),
            Some(k) => {
                if self.cases[k].a_out.is_some() {
                    return Err(JB_RERUN);
                }
                self.cases[k].a_out = Some(ok);
                Ok(())
            }
        }
    }

    /// 乙侧执行落账。
    pub fn b_run(&mut self, id: u32, ok: bool) -> Result<(), u32> {
        match self.pos(id) {
            None => Err(JB_NOCASE),
            Some(k) => {
                if self.cases[k].b_out.is_some() {
                    return Err(JB_RERUN);
                }
                self.cases[k].b_out = Some(ok);
                Ok(())
            }
        }
    }

    /// 联合裁断单条：未知样例或半跑均拒。
    pub fn joint(&self, id: u32) -> Result<bool, u32> {
        match self.pos(id) {
            None => Err(JB_NOCASE),
            Some(k) => {
                let c = &self.cases[k];
                if !c.both_ran() {
                    return Err(JB_HALF);
                }
                Ok(c.joint_pass())
            }
        }
    }

    /// 跑批完成：全部样例双侧同跑。
    pub fn batch_done(&self) -> bool {
        self.cases.len() >= CASES_NEEDED && self.cases.iter().all(|c| c.both_ran())
    }

    /// 异裁样例清单。
    pub fn conflicts(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].conflict() {
                v.push(self.cases[k].case_id);
            }
            k += 1;
        }
        v
    }

    /// 联合通过计数。
    pub fn passed_count(&self) -> usize {
        self.cases.iter().filter(|c| c.joint_pass()).count()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 跑批折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b0e00;
        h = h.wrapping_mul(31).wrapping_add(self.cases.len() as u32);
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = h.wrapping_mul(31).wrapping_add(c.case_id);
            h = h.wrapping_mul(31).wrapping_add(c.expect as u32);
            let va = c.a_out.unwrap_or(false) as u32 + c.a_out.is_some() as u32;
            let vb = c.b_out.unwrap_or(false) as u32 + c.b_out.is_some() as u32;
            h = h.wrapping_mul(31).wrapping_add(va * 4 + vb);
            let bytes = c.domain_tag.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = h.wrapping_mul(31).wrapping_add(bytes[b] as u32);
                b += 1;
            }
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 半跑（只甲侧）不可裁
    let mut r = JointRunner::new();
    let _ = r.submit(1, "半跑样例", true);
    let _ = r.a_run(1, true);
    got[0] = r.joint(1).err() == Some(JB_HALF);
    // 2) 异裁必判不通过且可枚举
    let _ = r.b_run(1, false);
    got[1] = r.joint(1) == Ok(false) && r.conflicts() == [1u32];
    // 3) 未知样例执行必拒
    got[2] = r.a_run(9, true).err() == Some(JB_NOCASE)
        && r.joint(9).err() == Some(JB_NOCASE);
    // 4) 重跑必拒
    got[3] = r.a_run(1, true).err() == Some(JB_RERUN);
    got
}

/// 批内判据。
pub fn run_unx10307_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10307");

    // J1 正路径：八样例双侧同跑同裁合预期
    let mut r = JointRunner::new();
    let mut n = 1u32;
    let mut ok = true;
    while n <= CASES_NEEDED as u32 {
        if r.submit(n, "跨域样例", true).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1u32;
    while n <= CASES_NEEDED as u32 {
        if r.a_run(n, true).is_err() || r.b_run(n, true).is_err() {
            ok = false;
        }
        n += 1;
    }
    s.add("J1-八样例联合通过", ok && r.batch_done() && r.passed_count() == CASES_NEEDED,
        "双侧同跑同裁合预期八条全过");

    // J2 逐条联合裁断
    let j2 = {
        let mut ok = true;
        let mut n = 1u32;
        while n <= CASES_NEEDED as u32 {
            if r.joint(n) != Ok(true) {
                ok = false;
            }
            n += 1;
        }
        ok
    };
    s.add("J2-逐条裁断", j2, "八条逐条联合裁断为真");

    // J3 注入检出：半跑 + 异裁
    let inj = inject_four();
    s.add("J3-半跑异裁检出", inj[0] && inj[1], "半跑不可裁且异裁必抓");

    // J4 注入检出：未知样例 + 重跑
    s.add("J4-未知重跑检出", inj[2] && inj[3], "未知样例与重跑必拒");

    // J5 预期为假的样例：双侧同裁假即联合通过
    let j5 = {
        let mut c = JointRunner::new();
        let _ = c.submit(1, "负样例", false);
        let _ = c.a_run(1, false);
        let _ = c.b_run(1, false);
        c.joint(1) == Ok(true)
    };
    s.add("J5-负样例合预期", j5, "预期假且双侧同裁假即通过");

    // J6 同裁但不合预期即不通过
    let j6 = {
        let mut c = JointRunner::new();
        let _ = c.submit(1, "错裁", true);
        let _ = c.a_run(1, false);
        let _ = c.b_run(1, false);
        c.joint(1) == Ok(false) && c.conflicts().is_empty()
    };
    s.add("J6-错裁不合预期", j6, "同裁假但预期真即不通过且不算异裁");

    // J7 重号拒
    let j7 = {
        let mut c = JointRunner::new();
        let _ = c.submit(1, "甲", true);
        c.submit(1, "乙", true).err() == Some(JB_DUP)
    };
    s.add("J7-重号拒收", j7, "样例号重号必拒");

    // J8 空域标拒
    let j8 = JointRunner::new().submit(1, "", true).err() == Some(JB_EMPTY);
    s.add("J8-空域标拒", j8, "域标空必拒");

    // J9 封批后提交拒
    let j9 = {
        let mut c = JointRunner::new();
        let _ = c.submit(1, "甲", true);
        c.seal();
        c.submit(2, "迟到", true).err() == Some(JB_SEALED)
    };
    s.add("J9-封批拒提", j9, "封批后提交必拒");

    // J10 容量边界
    let j10 = {
        let mut c = JointRunner::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            if c.submit(n, "x", true).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.submit(999, "over", true).err() == Some(JB_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原册不损");

    // J11 指纹随执行推进且终态稳定
    let j11 = {
        let mut c = JointRunner::new();
        let _ = c.submit(1, "指", true);
        let f0 = c.fold();
        let _ = c.a_run(1, true);
        let f1 = c.fold();
        let _ = c.b_run(1, true);
        f0 != f1 && c.fold() != f1 && c.fold() == c.fold()
    };
    s.add("J11-指纹随跑", j11, "每步执行指纹推进且终态稳定");

    // J12 动态明细：联合通过数落账
    let pc = r.passed_count();
    s.add_dyn("J12-通过计数", pc == CASES_NEEDED, &format!("passed={}", pc));

    // J13 动态明细：异裁零清单
    let cf = r.conflicts().len();
    s.add_dyn("J13-异裁零明细", cf == 0, &format!("conflicts={}", cf));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10307_batch_flow() {
        let mut r = JointRunner::new();
        assert!(r.submit(1, "a", true).is_ok());
        assert!(r.submit(1, "b", true).err() == Some(JB_DUP));
        assert!(r.submit(2, "", true).err() == Some(JB_EMPTY));
        assert!(r.a_run(9, true).err() == Some(JB_NOCASE));
        assert!(r.a_run(1, true).is_ok());
        assert!(r.a_run(1, true).err() == Some(JB_RERUN));
        assert!(r.joint(1).err() == Some(JB_HALF));
        assert!(r.b_run(1, true).is_ok());
        assert_eq!(r.joint(1), Ok(true));
        r.seal();
        assert!(r.submit(3, "late", true).err() == Some(JB_SEALED));
    }

    #[test]
    fn unx10307_checks_green() {
        let s = run_unx10307_checks();
        assert!(s.all_passed(), "UNX-F10307 判据未全绿");
    }
}
