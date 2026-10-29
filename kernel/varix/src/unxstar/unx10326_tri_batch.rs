//! UNX-F10326 联签样例跑批（三面联合执行）（0x1b340 段）
//!
//! 锚点：UNX-F10326-J1 三面联合执行——每样例由 pty/进程/基建三面各跑
//! 一次，三面输出与期望全等才算该样例联合通过，全批样例齐跑且零冲突
//! 才算跑批完成。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：样例册（行=样例号+期望+三面输出 Option）+ 三面执行核（重跑
//! 即拒、未知样例即拒）+ 批闸（样例数达锚且三面全跑方可封批）+
//! 注入核（重号/未知样例/重跑/超容四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b340 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b340;
pub const TB_ZERO: u32 = SEG_BASE + 0x01; // 样例号零
pub const TB_DUP: u32 = SEG_BASE + 0x02; // 样例号重号
pub const TB_NOCASE: u32 = SEG_BASE + 0x03; // 未知样例
pub const TB_RERUN: u32 = SEG_BASE + 0x04; // 已跑再跑
pub const TB_CAP: u32 = SEG_BASE + 0x05; // 批超容
pub const TB_SEALED: u32 = SEG_BASE + 0x06; // 封批后跑
pub const TB_NOTREADY: u32 = SEG_BASE + 0x07; // 未齐即封批

/// 样例需求锚（六例）。
pub const CASES_NEEDED: usize = 6;
/// 批定容。
pub const CAP: usize = 10;

/// 三面样例行。
#[derive(Clone)]
pub struct TriCase {
    pub case_id: u32,
    pub expect: u32,
    pub pty_out: Option<u32>,
    pub proc_out: Option<u32>,
    pub infra_out: Option<u32>,
}

impl TriCase {
    /// 三面齐跑。
    pub fn all_run(&self) -> bool {
        self.pty_out.is_some() && self.proc_out.is_some() && self.infra_out.is_some()
    }

    /// 联合通过：三面输出与期望全等。
    pub fn joint(&self) -> bool {
        self.all_run()
            && self.pty_out.unwrap_or(0) == self.expect
            && self.proc_out.unwrap_or(0) == self.expect
            && self.infra_out.unwrap_or(0) == self.expect
    }
}

/// 三面联签跑批核。
pub struct TriRunner {
    pub cases: Vec<TriCase>,
    pub sealed: bool,
}

impl TriRunner {
    pub fn new() -> TriRunner {
        TriRunner { cases: Vec::new(), sealed: false }
    }

    fn pos(&self, cid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].case_id == cid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 提交样例。
    pub fn submit(&mut self, cid: u32, expect: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TB_SEALED);
        }
        if cid == 0 {
            return Err(TB_ZERO);
        }
        if self.pos(cid).is_some() {
            return Err(TB_DUP);
        }
        if self.cases.len() >= CAP {
            return Err(TB_CAP);
        }
        self.cases.push(TriCase {
            case_id: cid,
            expect,
            pty_out: None,
            proc_out: None,
            infra_out: None,
        });
        Ok(())
    }

    fn run_face(&mut self, cid: u32, out: u32, face: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(TB_SEALED);
        }
        match self.pos(cid) {
            None => Err(TB_NOCASE),
            Some(p) => {
                let slot = match face {
                    0 => &mut self.cases[p].pty_out,
                    1 => &mut self.cases[p].proc_out,
                    _ => &mut self.cases[p].infra_out,
                };
                if slot.is_some() {
                    return Err(TB_RERUN);
                }
                *slot = Some(out);
                Ok(())
            }
        }
    }

    /// pty 面执行。
    pub fn run_pty(&mut self, cid: u32, out: u32) -> Result<(), u32> {
        self.run_face(cid, out, 0)
    }

    /// 进程面执行。
    pub fn run_proc(&mut self, cid: u32, out: u32) -> Result<(), u32> {
        self.run_face(cid, out, 1)
    }

    /// 基建面执行。
    pub fn run_infra(&mut self, cid: u32, out: u32) -> Result<(), u32> {
        self.run_face(cid, out, 2)
    }

    /// 冲突样例清单（任一面输出异于期望）。
    pub fn conflicts(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            if c.all_run() && !c.joint() {
                v.push(c.case_id);
            }
            k += 1;
        }
        v
    }

    /// 未跑全清单。
    pub fn not_run(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.cases.len() {
            if !self.cases[k].all_run() {
                v.push(self.cases[k].case_id);
            }
            k += 1;
        }
        v
    }

    /// 联合通过样例数。
    pub fn passed_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            if self.cases[k].joint() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 跑批完成：样例数达锚、全跑全过零冲突。
    pub fn batch_done(&self) -> bool {
        self.cases.len() >= CASES_NEEDED
            && self.not_run().is_empty()
            && self.conflicts().is_empty()
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(TB_SEALED);
        }
        if !self.batch_done() {
            return Err(TB_NOTREADY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 批指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b3400;
        h = h.wrapping_mul(31).wrapping_add(self.cases.len() as u32);
        let mut k = 0usize;
        while k < self.cases.len() {
            let c = &self.cases[k];
            h = h.wrapping_mul(31).wrapping_add(c.case_id);
            h = h.wrapping_mul(31).wrapping_add(c.expect);
            h = h.wrapping_mul(31).wrapping_add(c.pty_out.unwrap_or(0) + 1);
            h = h.wrapping_mul(31).wrapping_add(c.proc_out.unwrap_or(0) + 1);
            h = h.wrapping_mul(31).wrapping_add(c.infra_out.unwrap_or(0) + 1);
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut r = TriRunner::new();
    // 1) 零号与重号必拒
    let _ = r.submit(1, 7);
    got[0] = r.submit(0, 7).err() == Some(TB_ZERO)
        && r.submit(1, 7).err() == Some(TB_DUP);
    // 2) 未知样例执行必拒
    got[1] = r.run_pty(99, 7).err() == Some(TB_NOCASE);
    // 3) 重跑必拒
    let _ = r.run_pty(1, 7);
    got[2] = r.run_pty(1, 7).err() == Some(TB_RERUN);
    // 4) 冲突样例必入冲突单（期望 7 进程面出 8）
    let _ = r.run_proc(1, 8);
    let _ = r.run_infra(1, 7);
    got[3] = r.conflicts() == [1u32] && !r.cases[0].joint();
    got
}

/// 批内判据。
pub fn run_unx10326_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10326");

    // J1 正路径：六样例三面联合执行全过并封批
    let mut r = TriRunner::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= CASES_NEEDED as u32 {
        if r.submit(n, n * 10).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1;
    while n <= CASES_NEEDED as u32 {
        let e = n * 10;
        if r.run_pty(n, e).is_err() || r.run_proc(n, e).is_err() || r.run_infra(n, e).is_err() {
            ok = false;
        }
        n += 1;
    }
    let sealed = r.seal();
    s.add(
        "J1-三面联合执行",
        ok && r.batch_done() && sealed.is_ok() && r.sealed,
        "六样例三面输出与期望全等并成功封批",
    );

    // J2 联合通过计数满六
    s.add(
        "J2-通过计数",
        r.passed_count() == CASES_NEEDED,
        "六样例全联合通过",
    );

    // J3 注入检出：零号重号 + 未知样例
    let inj = inject_four();
    s.add("J3-重号未知检出", inj[0] && inj[1], "零号重号与未知样例必拒");

    // J4 注入检出：重跑 + 冲突入单
    s.add("J4-重跑冲突检出", inj[2] && inj[3], "重跑必拒且冲突样例必入单");

    // J5 冲突阻断封批
    let j5 = {
        let mut t = TriRunner::new();
        let mut k = 1u32;
        while k <= CASES_NEEDED as u32 {
            let _ = t.submit(k, 5);
            let _ = t.run_pty(k, 5);
            let _ = t.run_proc(k, 5);
            let _ = t.run_infra(k, 5);
            k += 1;
        }
        let _ = t.run_proc(3, 6);
        !t.batch_done()
    };
    s.add("J5-冲突阻断", j5, "任一面输出异于期望即不成批");

    // J6 缺跑阻断封批（第三例漏跑基建面）
    let j6 = {
        let mut t = TriRunner::new();
        let mut k = 1u32;
        while k <= CASES_NEEDED as u32 {
            let _ = t.submit(k, 1);
            let _ = t.run_pty(k, 1);
            let _ = t.run_proc(k, 1);
            if k != 3 {
                let _ = t.run_infra(k, 1);
            }
            k += 1;
        }
        t.not_run().contains(&3) && t.seal().err() == Some(TB_NOTREADY)
    };
    s.add("J6-缺跑阻断", j6, "任一样例三面缺一即不成批");

    // J7 封批后执行必拒
    let j7 = {
        let mut t = TriRunner::new();
        let mut k = 1u32;
        while k <= CASES_NEEDED as u32 {
            let _ = t.submit(k, 2);
            let _ = t.run_pty(k, 2);
            let _ = t.run_proc(k, 2);
            let _ = t.run_infra(k, 2);
            k += 1;
        }
        let _ = t.seal();
        t.run_pty(1, 2).err() == Some(TB_SEALED)
            && t.submit(9, 9).err() == Some(TB_SEALED)
    };
    s.add("J7-封批拒动", j7, "封批后提交与执行均拒");

    // J8 容量边界
    let j8 = {
        let mut t = TriRunner::new();
        let mut k = 1u32;
        let mut filled = true;
        while k <= CAP as u32 {
            if t.submit(k, k).is_err() {
                filled = false;
            }
            k += 1;
        }
        filled && t.submit(999, 0).err() == Some(TB_CAP)
    };
    s.add("J8-容量边界", j8, "超容必拒且原批不损");

    // J9 指纹随执行推进
    let j9 = {
        let mut t = TriRunner::new();
        let _ = t.submit(1, 4);
        let f0 = t.fold();
        let _ = t.run_pty(1, 4);
        let f1 = t.fold();
        let _ = t.run_proc(1, 4);
        let _ = t.run_infra(1, 4);
        let f3 = t.fold();
        f0 != f1 && f1 != f3 && t.fold() == f3
    };
    s.add("J9-指纹推进", j9, "每跑一步指纹一步且终态稳定");

    // J10 未跑清单按序可枚举
    let j10 = {
        let mut t = TriRunner::new();
        let mut k = 1u32;
        while k <= 4u32 {
            let _ = t.submit(k, 0);
            let _ = t.run_pty(k, 0);
            k += 1;
        }
        t.not_run() == [1u32, 2, 3, 4]
    };
    s.add("J10-未跑枚举", j10, "未跑全样例按登记序可枚举");

    // J11 联合通过判据逐面核对
    let j11 = {
        let mut t = TriRunner::new();
        let _ = t.submit(1, 9);
        let _ = t.run_pty(1, 9);
        let _ = t.run_proc(1, 9);
        let _ = t.run_infra(1, 9);
        let c0 = &t.cases[0];
        c0.pty_out == Some(9) && c0.proc_out == Some(9) && c0.infra_out == Some(9)
    };
    s.add("J11-逐面核对", j11, "三面输出槽逐面与期望核对");

    // J12 动态明细：通过计数落账
    let pc = r.passed_count();
    s.add_dyn(
        "J12-通过计数",
        pc == CASES_NEEDED,
        &format!("passed={}/{}", pc, r.cases.len()),
    );

    // J13 动态明细：批指纹落账
    let fp = r.fold();
    s.add_dyn("J13-批指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10326_tri_batch_flow() {
        let mut r = TriRunner::new();
        assert!(r.submit(0, 1).err() == Some(TB_ZERO));
        assert!(r.submit(1, 7).is_ok());
        assert!(r.submit(1, 7).err() == Some(TB_DUP));
        assert!(r.run_pty(99, 7).err() == Some(TB_NOCASE));
        assert!(r.run_pty(1, 7).is_ok());
        assert!(r.run_pty(1, 7).err() == Some(TB_RERUN));
        assert!(r.run_proc(1, 8).is_ok());
        assert!(r.run_infra(1, 7).is_ok());
        assert!(r.conflicts().contains(&1));
        assert!(!r.cases[0].joint());
        assert!(r.seal().err() == Some(TB_NOTREADY));
        let mut k = 2u32;
        while k <= CASES_NEEDED as u32 {
            let _ = r.submit(k, 3);
            let _ = r.run_pty(k, 3);
            let _ = r.run_proc(k, 3);
            let _ = r.run_infra(k, 3);
            k += 1;
        }
        assert!(r.batch_done());
        assert!(r.seal().is_ok());
        assert!(r.run_proc(2, 3).err() == Some(TB_SEALED));
    }

    #[test]
    fn unx10326_checks_green() {
        let s = run_unx10326_checks();
        assert!(s.all_passed(), "UNX-F10326 判据未全绿");
    }
}
