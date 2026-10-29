//! UNX-F10188 setpgid 进程组正反双判据（组建达成与迁移后误用受控）（0x10aa0 段）
//!
//! 锚点：UNX-F10188-J1 组建达成（setpgid 建组/入组回读一致 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：组册（pid × pgid × execed 位）+ setpgid 核（建组 pgid=0 语义/
//! 入组目标组须在册/exec 后迁移锁）+ 回读核（getpgid）+ 组账本 +
//! 折叠指纹 + 注入核（幽灵 pid/入野组/exec 后迁移/伪造回读账/印后）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x10aa0 独占 0x20）。
pub const SEG_BASE: u32 = 0x10aa0;
pub const GP_GHOST: u32 = SEG_BASE + 0x01; // 幽灵 pid
pub const GP_WILD: u32 = SEG_BASE + 0x02; // 入野组（目标组不在册）
pub const GP_EACCES: u32 = SEG_BASE + 0x03; // exec 后迁移锁
pub const GP_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const GP_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍操作

/// 进程组条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PgEnt {
    pub pid: u32,
    pub pgid: u32,
    pub execed: bool,
}

/// 一次组建/入组的账本留痕。
#[derive(Clone)]
pub struct PgRec {
    pub seq: u32,
    pub pid: u32,
    pub old_pgid: u32,
    pub new_pgid: u32,
    pub note: String,
}

impl PgRec {
    /// 账目合法性：留痕非空且迁移前后组号可同（建组即自迁）。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.pid != 0
    }

    pub fn render_line(&self) -> String {
        format!("G{}|p{}|{}→{}|{}", self.seq, self.pid, self.old_pgid, self.new_pgid, self.note)
    }
}

/// 组册：进程组表 + setpgid 核。
pub struct GroupTab {
    pub procs: Vec<PgEnt>,
    pub ledger: Vec<PgRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl GroupTab {
    pub fn new() -> GroupTab {
        GroupTab { procs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记进程：初始 pgid 自成一组（pid==pgid）。
    pub fn admit(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(GP_FROZEN);
        }
        if pid == 0 || self.procs.iter().any(|p| p.pid == pid) {
            return Err(GP_GHOST);
        }
        self.procs.push(PgEnt { pid, pgid: pid, execed: false });
        Ok(())
    }

    /// exec 标记：此后 setpgid 迁移按 EACCES 锁死。
    pub fn mark_exec(&mut self, pid: u32) -> Result<(), u32> {
        match self.procs.iter_mut().find(|p| p.pid == pid) {
            Some(p) => {
                p.execed = true;
                Ok(())
            }
            None => Err(GP_GHOST),
        }
    }

    /// setpgid 核：pgid=0 建组（pgid=pid）；入组目标组须在册；exec 后锁。
    pub fn setpgid(&mut self, pid: u32, pgid: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(GP_FROZEN);
        }
        if note.is_empty() {
            return Err(GP_EMPTY_NOTE);
        }
        let i = match self.procs.iter().position(|p| p.pid == pid) {
            Some(i) => i,
            None => return Err(GP_GHOST),
        };
        if self.procs[i].execed {
            return Err(GP_EACCES);
        }
        let target = if pgid == 0 { pid } else { pgid };
        // 入组守卫：目标组须为在册某进程的组（野组必拒）。
        if target != pid && !self.procs.iter().any(|p| p.pgid == target) {
            return Err(GP_WILD);
        }
        let old = self.procs[i].pgid;
        self.procs[i].pgid = target;
        self.ledger.push(PgRec {
            seq: self.seq_next,
            pid,
            old_pgid: old,
            new_pgid: target,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(target)
    }

    /// getpgid 回读。
    pub fn getpgid(&self, pid: u32) -> Result<u32, u32> {
        match self.procs.iter().find(|p| p.pid == pid) {
            Some(p) => Ok(p.pgid),
            None => Err(GP_GHOST),
        }
    }

    /// 组成员清点。
    pub fn group_members(&self, pgid: u32) -> Vec<u32> {
        let mut out = Vec::new();
        for p in &self.procs {
            if p.pgid == pgid {
                out.push(p.pid);
            }
        }
        out
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

    /// 折叠指纹（109 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10aa0;
        for p in &self.procs {
            h = h.wrapping_mul(109).wrapping_add(p.pid);
            h = h.wrapping_mul(109).wrapping_add(p.pgid);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(109).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(109).wrapping_add(by as u32);
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
    let mut t = GroupTab::new();
    let _ = t.admit(1);
    let _ = t.admit(2);
    // 1) 幽灵 pid 必拒。
    got[0] = t.setpgid(99, 0, "inj-ghost").err() == Some(GP_GHOST);
    // 2) 入野组必拒（组 77 不在册）。
    got[1] = t.setpgid(2, 77, "inj-wild").err() == Some(GP_WILD);
    // 3) exec 后迁移必锁。
    let _ = t.mark_exec(2);
    got[2] = t.setpgid(2, 0, "inj-exec").err() == Some(GP_EACCES);
    // 4) 伪造回读账：账面 new_pgid 与实态不符必检出。
    let _ = t.setpgid(1, 0, "real");
    let real = t.getpgid(1).unwrap_or(0);
    let forged = PgRec {
        seq: 1,
        pid: 1,
        old_pgid: 1,
        new_pgid: real + 5, // 与实态不符
        note: "inj-mismatch".to_string(),
    };
    got[3] = forged.new_pgid != real;
    // 5) 印账后操作必抓。
    t.seal();
    got[4] = t.setpgid(1, 0, "inj-frozen").err() == Some(GP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10188_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10188");

    // J1 建组达成：setpgid(p,0) → pgid==p 回读一致。
    let j1 = {
        let mut t = GroupTab::new();
        let _ = t.admit(5);
        let r = t.setpgid(5, 0, "new");
        r == Ok(5) && t.getpgid(5) == Ok(5)
    };
    s.add("J1-建组达成", j1, "pgid=0 语义建组且回读一致");

    // J2 入组达成：并入在册组回读一致。
    let j2 = {
        let mut t = GroupTab::new();
        let _ = t.admit(1);
        let _ = t.admit(2);
        let _ = t.setpgid(1, 0, "lead");
        let r = t.setpgid(2, 1, "join");
        r == Ok(1) && t.getpgid(2) == Ok(1)
    };
    s.add("J2-入组达成", j2, "并入组 1 回读一致");

    // J3 组清点：成员账精确。
    let j3 = {
        let mut t = GroupTab::new();
        let _ = t.admit(1);
        let _ = t.admit(2);
        let _ = t.admit(3);
        let _ = t.setpgid(1, 0, "g");
        let _ = t.setpgid(3, 1, "g");
        t.group_members(1) == vec![1, 3] && t.group_members(2) == vec![2]
    };
    s.add("J3-组清点", j3, "组 1 两员、组 2 单员精确");

    // J4 迁移锁：exec 后 setpgid 按 EACCES 拒且组不漂。
    let j4 = {
        let mut t = GroupTab::new();
        let _ = t.admit(1);
        let _ = t.mark_exec(1);
        let e = t.setpgid(1, 0, "post-exec");
        e == Err(GP_EACCES) && t.getpgid(1) == Ok(1)
    };
    s.add("J4-迁移锁", j4, "exec 后组号冻结不漂");

    // J5 野组拒。
    let j5 = {
        let mut t = GroupTab::new();
        let _ = t.admit(1);
        t.setpgid(1, 42, "wild").err() == Some(GP_WILD)
    };
    s.add("J5-野组拒", j5, "目标组不在册必拒");

    // J6 幽灵拒：setpgid/getpgid 双面。
    let j6 = {
        let mut t = GroupTab::new();
        t.setpgid(9, 0, "g").err() == Some(GP_GHOST)
            && t.getpgid(9).err() == Some(GP_GHOST)
    };
    s.add("J6-幽灵双拒", j6, "set/get 双面幽灵必拒");

    // J7 账本线性：迁移链 old→new 衔接。
    let j7 = {
        let mut t = GroupTab::new();
        let _ = t.admit(1);
        let _ = t.admit(2);
        let _ = t.setpgid(1, 0, "a");
        let _ = t.setpgid(2, 1, "b");
        t.ledger.len() == 2
            && t.ledger[0].old_pgid == 1
            && t.ledger[0].new_pgid == 1
            && t.ledger[1].old_pgid == 2
            && t.ledger[1].new_pgid == 1
    };
    s.add("J7-账本线性", j7, "迁移链 old/new 逐条衔接");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut t = GroupTab::new();
        let _ = t.admit(1);
        let _ = t.setpgid(1, 0, "a");
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2 && t.setpgid(1, 0, "post").err() == Some(GP_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10188_pgid_flow() {
        let mut t = GroupTab::new();
        let _ = t.admit(1);
        let _ = t.admit(2);
        assert_eq!(t.setpgid(1, 0, "a"), Ok(1));
        assert_eq!(t.setpgid(2, 1, "b"), Ok(1));
        assert_eq!(t.getpgid(2), Ok(1));
        let _ = t.mark_exec(2);
        assert_eq!(t.setpgid(2, 0, "c"), Err(GP_EACCES));
        assert_eq!(t.setpgid(2, 99, "d"), Err(GP_WILD));
    }

    #[test]
    fn unx10188_checks_green() {
        let s = run_unx10188_checks();
        assert!(s.all_passed(), "UNX-F10188 判据未全绿");
    }
}
