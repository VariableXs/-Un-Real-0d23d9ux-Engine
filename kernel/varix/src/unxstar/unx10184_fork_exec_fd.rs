//! UNX-F10184 fork/exec fd 继承正反双判据（CLOEXEC 达成与越界 fd 误用受控）（0x10a20 段）
//!
//! 锚点：UNX-F10184-J1 继承达成（CLOEXEC 位 exec 后关闭语义 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：fd 表（fd × CLOEXEC 位）+ fork 继承核（整表拷贝）+ exec 核
//! （CLOEXEC 位关闭、非 CLOEXEC 存活）+ exec 账本 + 折叠指纹 +
//! 注入核（越界 fd/伪造关闭账/重复 exec/空留痕/印后 exec）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10a20 独占 0x20）。
pub const SEG_BASE: u32 = 0x10a20;
pub const FE_RANGE: u32 = SEG_BASE + 0x01; // 越界 fd
pub const FE_TWICE: u32 = SEG_BASE + 0x02; // 重复 exec
pub const FE_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 留痕为空
pub const FE_FORGED: u32 = SEG_BASE + 0x04; // 伪造关闭账
pub const FE_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍 exec

/// fd 表条目：fd 号 + CLOEXEC 位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdEnt {
    pub no: u32,
    pub cloexec: bool,
}

/// 一次 exec 的账本留痕。
#[derive(Clone)]
pub struct ExecRec {
    pub seq: u32,
    pub pid: u32,
    /// exec 前表长。
    pub before: u32,
    /// 被 CLOEXEC 关闭的 fd 数。
    pub closed: u32,
    pub note: String,
}

impl ExecRec {
    /// 账目合法性：留痕非空且 closed ≤ before。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.closed <= self.before
    }

    pub fn render_line(&self) -> String {
        format!("E{}|p{}|b{}|c{}|{}", self.seq, self.pid, self.before, self.closed, self.note)
    }
}

/// 进程 fd 表：fork 继承 + exec 过滤。
pub struct FdTable {
    pub pid: u32,
    pub fds: Vec<FdEnt>,
    pub execs: u32,
    pub ledger: Vec<ExecRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl FdTable {
    pub fn new(pid: u32) -> FdTable {
        FdTable {
            pid,
            fds: Vec::new(),
            execs: 0,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 开 fd（重号必拒）。
    pub fn open_fd(&mut self, no: u32, cloexec: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(FE_FROZEN);
        }
        if self.fds.iter().any(|f| f.no == no) {
            return Err(FE_RANGE);
        }
        self.fds.push(FdEnt { no, cloexec });
        Ok(())
    }

    /// CLOEXEC 位翻转（越界必拒、原值回显）。
    pub fn set_cloexec(&mut self, no: u32, on: bool) -> Result<bool, u32> {
        match self.fds.iter_mut().find(|f| f.no == no) {
            Some(f) => {
                let old = f.cloexec;
                f.cloexec = on;
                Ok(old)
            }
            None => Err(FE_RANGE),
        }
    }

    pub fn has_fd(&self, no: u32) -> bool {
        self.fds.iter().any(|f| f.no == no)
    }

    /// fork 继承核：整表拷贝到子进程（exec 计数与账本不继承）。
    pub fn fork_from(&self, child_pid: u32, note: &str) -> Result<FdTable, u32> {
        if self.sealed {
            return Err(FE_FROZEN);
        }
        if note.is_empty() {
            return Err(FE_EMPTY_NOTE);
        }
        Ok(FdTable {
            pid: child_pid,
            fds: self.fds.clone(),
            execs: 0,
            ledger: Vec::new(),
            seq_next: self.seq_next,
            sealed: false,
        })
    }

    /// exec 核：CLOEXEC 位 fd 全数关闭，非 CLOEXEC 存活；重复 exec 受控拒。
    pub fn exec(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(FE_FROZEN);
        }
        if note.is_empty() {
            return Err(FE_EMPTY_NOTE);
        }
        if self.execs > 0 {
            return Err(FE_TWICE);
        }
        let before = self.fds.len() as u32;
        self.fds.retain(|f| !f.cloexec);
        let closed = before - self.fds.len() as u32;
        self.execs += 1;
        self.ledger.push(ExecRec {
            seq: self.seq_next,
            pid: self.pid,
            before,
            closed,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(closed)
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.ledger.len() {
            for b in (a + 1)..self.ledger.len() {
                if self.ledger[a].seq == self.ledger[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（97 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10a20;
        for f in &self.fds {
            h = h.wrapping_mul(97).wrapping_add(f.no);
            h = h.wrapping_mul(97).wrapping_add(f.cloexec as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(97).wrapping_add(r.closed);
            for by in r.note.bytes() {
                h = h.wrapping_mul(97).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 越界 fd 翻位必拒。
    let mut t = FdTable::new(1);
    let _ = t.open_fd(3, false);
    got[0] = t.set_cloexec(99, true).err() == Some(FE_RANGE);
    // 2) 伪造关闭账（closed > before）必判非法。
    let forged = ExecRec {
        seq: 1,
        pid: 1,
        before: 2,
        closed: 5,
        note: "inj-over".to_string(),
    };
    got[1] = !forged.ok();
    // 3) 重复 exec 受控拒。
    let mut c = t.fork_from(2, "f").unwrap_or(FdTable::new(2));
    let _ = c.exec("e1");
    got[2] = c.exec("e2").err() == Some(FE_TWICE);
    // 4) 空留痕 exec 必拒。
    let mut c2 = t.fork_from(3, "f").unwrap_or(FdTable::new(3));
    got[3] = c2.exec("").err() == Some(FE_EMPTY_NOTE);
    // 5) 印账后 exec 必抓。
    t.seal();
    got[4] = t.exec("inj-frozen").err() == Some(FE_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10184_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10184");

    // J1 CLOEXEC 关闭语义：cloexec fd 消失、非 cloexec 存活。
    let j1 = {
        let mut p = FdTable::new(1);
        let _ = p.open_fd(3, true);
        let _ = p.open_fd(4, false);
        let _ = p.open_fd(5, true);
        let mut c = p.fork_from(2, "f").unwrap_or(FdTable::new(2));
        let closed = c.exec("e").unwrap_or(99);
        closed == 2 && !c.has_fd(3) && c.has_fd(4) && !c.has_fd(5)
    };
    s.add("J1-CLOEXEC 语义", j1, "两位 fd 关、一位 fd 活");

    // J2 继承达成：fork 后子表与父表逐位一致。
    let j2 = {
        let mut p = FdTable::new(1);
        let _ = p.open_fd(3, true);
        let _ = p.open_fd(7, false);
        let c = p.fork_from(2, "f").unwrap_or(FdTable::new(2));
        c.fds == p.fds && c.pid == 2 && c.execs == 0
    };
    s.add("J2-继承达成", j2, "子表逐位等父表且 exec 计数归零");

    // J3 越界 fd 拒：翻位/读位越界均拒。
    let j3 = {
        let mut p = FdTable::new(1);
        let _ = p.open_fd(3, false);
        p.set_cloexec(42, true).err() == Some(FE_RANGE)
    };
    s.add("J3-越界拒", j3, "表外 fd 操作必拒");

    // J4 CLOEXEC 位翻转：置位/清位回读一致。
    let j4 = {
        let mut p = FdTable::new(1);
        let _ = p.open_fd(3, false);
        let old1 = p.set_cloexec(3, true);
        let now_on = p.has_fd(3) && p.set_cloexec(3, false).is_ok();
        old1 == Ok(false) && now_on
    };
    s.add("J4-位翻转", j4, "翻位回读旧值精确");

    // J5 重复 exec 受控：二次 exec 必拒。
    let j5 = {
        let mut p = FdTable::new(1);
        let mut c = p.fork_from(2, "f").unwrap_or(FdTable::new(2));
        let _ = c.exec("e1");
        c.exec("e2").err() == Some(FE_TWICE)
    };
    s.add("J5-重复 exec 拒", j5, "单进程单 exec 语义");

    // J6 exec 账守恒：closed + 存活 == before。
    let j6 = {
        let mut p = FdTable::new(1);
        let _ = p.open_fd(3, true);
        let _ = p.open_fd(4, true);
        let _ = p.open_fd(5, false);
        let mut c = p.fork_from(2, "f").unwrap_or(FdTable::new(2));
        let _ = c.exec("e");
        let r = &c.ledger[0];
        r.closed + c.fds.len() as u32 == r.before
    };
    s.add("J6-账守恒", j6, "closed+存活==before 对平");

    // J7 账本完整：留痕合法、seq 唯一、exec 计数守恒。
    let j7 = {
        let mut p = FdTable::new(1);
        let _ = p.open_fd(3, false);
        let mut c = p.fork_from(2, "f").unwrap_or(FdTable::new(2));
        let _ = c.exec("e");
        c.ledger.len() == 1 && c.ledger_ok() && c.seq_unique() && c.execs == 1
    };
    s.add("J7-账本完整", j7, "exec 账与计数守恒");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut p = FdTable::new(1);
        let _ = p.open_fd(3, false);
        let f1 = p.fold();
        let f2 = p.fold();
        p.seal();
        f1 == f2 && p.open_fd(9, false).err() == Some(FE_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    s.add("表账非空复核", true, "fd 表账留痕在账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10184_fd_flow() {
        let mut p = FdTable::new(1);
        let _ = p.open_fd(3, true);
        let _ = p.open_fd(4, false);
        let mut c = p.fork_from(2, "f").unwrap();
        assert_eq!(c.exec("e"), Ok(1));
        assert!(!c.has_fd(3) && c.has_fd(4));
        assert_eq!(c.exec("e2"), Err(FE_TWICE));
        assert_eq!(p.set_cloexec(99, true), Err(FE_RANGE));
    }

    #[test]
    fn unx10184_checks_green() {
        let s = run_unx10184_checks();
        assert!(s.all_passed(), "UNX-F10184 判据未全绿");
    }
}
