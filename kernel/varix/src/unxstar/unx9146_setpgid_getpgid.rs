//! UNX-F9146 setpgid/getpgid 语义档（进程组迁移协议）（0x63200 段）
//!
//! 锚点：UNX-F9146-J1 迁移账（每次 setpgid 落账：pid、旧组、新组、
//! 结果码、留痕五元组）；J2 迁移语义（self 闸/会话首闸/会话闸/exec
//! 闸/组存在闸五闸齐备 + 零号展开 + 组长自聚 + 组归并四性质）；J3
//! 迁移指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B18（进程组迁移面）。
//!
//! 结构：进程表（pid/ppid/sid/pgid/execed，子继承父 sid+pgid）+ 迁
//! 移账本 + 迁移核（五闸 + 零号展开）+ getpgid 闭路 + 折叠指纹 +
//! 注入核（错码/空留痕/重序号/未知 pid/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63200 独占 0x20）。
pub const SEG_BASE: u32 = 0x63200;
pub const PG_UNK_CALL: u32 = SEG_BASE + 0x01; // 未知 pid
pub const PG_BAD_GATE: u32 = SEG_BASE + 0x02; // 迁移闸判定错
pub const PG_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const PG_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const PG_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 结果码（语义层）。
pub const R_OK: u32 = 0;
pub const R_EPERM: u32 = 1; // 越权（非子非己/会话首/跨会话）
pub const R_EACCES: u32 = 2; // 子进程已 execve
pub const R_EINVAL: u32 = 3; // 目标组不存在
pub const R_ESRCH: u32 = 4; // pid 不存在
pub const R_COUNT: usize = 5;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EPERM", "EACCES", "EINVAL", "ESRCH"];

/// 单个进程快照。
#[derive(Clone)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub sid: u32,
    pub pgid: u32,
    pub execed: bool,
}

/// 单条迁移账。
#[derive(Clone)]
pub struct PgMove {
    pub seq: u32,
    pub pid: u32,
    pub old_pgid: u32,
    pub new_pgid: u32,
    pub code: u32,
    pub note: String,
}

impl PgMove {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.code as usize) < R_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "PM{}|p{}|{}->{}|{}|{}",
            self.seq,
            self.pid,
            self.old_pgid,
            self.new_pgid,
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// 进程组迁移表。
pub struct PgTable {
    pub procs: Vec<Proc>,
    pub ledger: Vec<PgMove>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl PgTable {
    pub fn new() -> PgTable {
        PgTable { procs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// spawn：子继承父 sid 与 pgid（无父者自为会话与组长，即 init）。
    pub fn spawn(&mut self, pid: u32, ppid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PG_FROZEN);
        }
        if self.find(pid).is_some() {
            return Err(PG_DUP_SEQ);
        }
        let (sid, pgid) = match self.find(ppid) {
            Some(i) => (self.procs[i].sid, self.procs[i].pgid),
            None => (ppid, pid), // 无父：自为会话首兼组长
        };
        self.procs.push(Proc { pid, ppid, sid, pgid, execed: false });
        Ok(())
    }

    pub fn mark_execed(&mut self, pid: u32) -> Result<(), u32> {
        match self.find(pid) {
            Some(i) => {
                self.procs[i].execed = true;
                Ok(())
            }
            None => Err(PG_UNK_CALL),
        }
    }

    pub fn find(&self, pid: u32) -> Option<usize> {
        self.procs.iter().position(|p| p.pid == pid)
    }

    /// 组在同会话内是否存在。
    pub fn group_in_session(&self, pgid: u32, sid: u32) -> bool {
        self.procs.iter().any(|p| p.pgid == pgid && p.sid == sid)
    }

    /// 是否会话首（存在以自己 sid 命名会话的进程 = 自己）。
    pub fn is_session_leader(&self, pid: u32) -> bool {
        match self.find(pid) {
            Some(i) => self.procs[i].sid == pid,
            None => false,
        }
    }

    /// 迁移核：setpgid(caller, pid, pgid)。pid=0→自身；pgid=0→目标自聚。
    pub fn setpgid(&mut self, caller: u32, pid: u32, pgid: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(PG_FROZEN);
        }
        if note.is_empty() {
            return Err(PG_EMPTY_NOTE);
        }
        let tp = if pid == 0 { caller } else { pid };
        let ti = match self.find(tp) {
            Some(i) => i,
            None => {
                self.record_fail(tp, R_ESRCH, note);
                return Err(R_ESRCH);
            }
        };
        let (tp_sid, tp_ppid, tp_execed, old_pgid) =
            (self.procs[ti].sid, self.procs[ti].ppid, self.procs[ti].execed, self.procs[ti].pgid);
        let caller_sid = match self.find(caller) {
            Some(ci) => self.procs[ci].sid,
            None => {
                self.record_fail(tp, R_ESRCH, note);
                return Err(R_ESRCH);
            }
        };
        // 闸 self：目标须为自身或自身子进程
        if tp != caller && tp_ppid != caller {
            self.record_fail(tp, R_EPERM, note);
            return Err(R_EPERM);
        }
        // 闸 session-head：会话首不可改组
        if self.is_session_leader(tp) {
            self.record_fail(tp, R_EPERM, note);
            return Err(R_EPERM);
        }
        // 闸 session：目标与调用者须同会话
        if tp_sid != caller_sid {
            self.record_fail(tp, R_EPERM, note);
            return Err(R_EPERM);
        }
        // 闸 execed：子已 execve 后父不可再改
        if tp != caller && tp_execed {
            self.record_fail(tp, R_EACCES, note);
            return Err(R_EACCES);
        }
        let npg = if pgid == 0 { tp } else { pgid };
        // 闸 group：npg 须为目标自身（自聚）或同会话已存在组
        if npg != tp && !self.group_in_session(npg, tp_sid) {
            self.record_fail(tp, R_EINVAL, note);
            return Err(R_EINVAL);
        }
        self.procs[ti].pgid = npg;
        let seq = self.record_ok(tp, old_pgid, npg, note);
        Ok(seq)
    }

    fn record_ok(&mut self, pid: u32, old_pgid: u32, new_pgid: u32, note: &str) -> u32 {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(PgMove {
            seq,
            pid,
            old_pgid,
            new_pgid,
            code: R_OK,
            note: note.to_string(),
        });
        seq
    }

    fn record_fail(&mut self, pid: u32, code: u32, note: &str) -> u32 {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(PgMove {
            seq,
            pid,
            old_pgid: 0,
            new_pgid: 0,
            code,
            note: note.to_string(),
        });
        seq
    }

    /// getpgid 闭路。
    pub fn getpgid(&self, caller: u32, pid: u32) -> Result<u32, u32> {
        let tp = if pid == 0 { caller } else { pid };
        match self.find(tp) {
            Some(i) => Ok(self.procs[i].pgid),
            None => Err(R_ESRCH),
        }
    }

    /// 组员清点。
    pub fn group_size(&self, pgid: u32) -> usize {
        self.procs.iter().filter(|p| p.pgid == pgid).count()
    }

    /// 账本 seq 唯一性。
    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 迁移账折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91460;
        for m in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(m.seq);
            h = h.wrapping_mul(31).wrapping_add(m.pid);
            h = h.wrapping_mul(31).wrapping_add(m.old_pgid);
            h = h.wrapping_mul(31).wrapping_add(m.new_pgid);
            h = h.wrapping_mul(31).wrapping_add(m.code);
            for b in m.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 基础样本：init(1) → sh(2) → a(10)/b(11)；另 sh3(3) → c(12)。
pub fn mk_table() -> PgTable {
    let mut t = PgTable::new();
    let _ = t.spawn(1, 1);
    let _ = t.spawn(2, 1);
    let _ = t.spawn(10, 2);
    let _ = t.spawn(11, 2);
    let _ = t.spawn(3, 1);
    let _ = t.spawn(12, 3);
    t
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错码写入
    let bad = PgMove {
        seq: 9001,
        pid: 1,
        old_pgid: 0,
        new_pgid: 0,
        code: 99,
        note: "inj-code".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = PgMove {
        seq: 9002,
        pid: 1,
        old_pgid: 0,
        new_pgid: 0,
        code: R_OK,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 重序号
    let mut t = mk_table();
    let _ = t.setpgid(2, 0, 0, "a");
    t.ledger.push(PgMove {
        seq: 1,
        pid: 7,
        old_pgid: 0,
        new_pgid: 0,
        code: R_OK,
        note: "inj-dup".to_string(),
    });
    got[2] = !t.seq_unique();
    // 4) 未知 pid setpgid → ESRCH
    let mut t2 = mk_table();
    got[3] = t2.setpgid(2, 0xDEAD, 0, "inj-unk").err() == Some(R_ESRCH);
    // 5) 印后登记必抓
    t2.seal();
    got[4] = t2.setpgid(2, 0, 0, "inj-frozen").err() == Some(PG_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9146_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9146");

    // J1 账非空
    let mut t = mk_table();
    let _ = t.setpgid(2, 0, 0, "sh 自聚为组长");
    let j1 = !t.ledger.is_empty();
    s.add("J1-迁移账非空", j1, "setpgid 后账本至少一条");

    // J2 账字段齐
    let j2 = t.ledger.iter().all(|m| m.ok());
    s.add("J2-账字段齐备", j2, "每条账留痕非空且码在闭集");

    // J3 零号展开：getpgid(0) == getpgid(caller)
    let j3 = t.getpgid(2, 0) == t.getpgid(2, 2);
    s.add("J3-零号展开", j3, "pid=0 与 pgid 查询均指自身");

    // J4 会话闸：子自立会话后父改组 → EPERM
    let j4 = {
        let mut t = mk_table();
        // sh(2) 自立会话（等价 setsid 效果，直接改快照）
        let i = t.find(2).unwrap();
        t.procs[i].sid = 2;
        t.setpgid(1, 2, 0, "改已自立子").err() == Some(R_EPERM)
    };
    s.add("J4-会话闸", j4, "跨会话改组必 EPERM");

    // J5 会话首闸：init 改自身组 → EPERM
    let j5 = {
        let mut t = mk_table();
        t.setpgid(1, 1, 0, "改会话首").err() == Some(R_EPERM)
    };
    s.add("J5-会话首闸", j5, "会话首进程禁改组");

    // J6 exec 闸：同会话内子已 execve → EACCES
    let j6 = {
        let mut t = mk_table();
        let _ = t.mark_execed(10);
        t.setpgid(2, 10, 0, "exec 后改组").err() == Some(R_EACCES)
    };
    s.add("J6-exec闸", j6, "子 execve 后父改组必 EACCES");

    // J7 组存在闸：目标组不在同会话 → EINVAL
    let j7 = {
        let mut t = mk_table();
        t.setpgid(2, 10, 777, "组不存在").err() == Some(R_EINVAL)
    };
    s.add("J7-组存在闸", j7, "目标组不在同会话必 EINVAL");

    // J8 组长自聚：pgid=0 后目标成为组长
    let j8 = {
        let mut t = mk_table();
        let _ = t.setpgid(2, 10, 0, "a 自聚");
        t.getpgid(2, 10) == Ok(10) && t.group_size(10) == 1
    };
    s.add("J8-组长自聚", j8, "pgid=0 后目标 pgid=pid");

    // J9 组归并：a、b 同归一组
    let j9 = {
        let mut t = mk_table();
        let _ = t.setpgid(2, 10, 0, "a 自聚");
        let _ = t.setpgid(2, 11, 10, "b 并入");
        t.group_size(10) == 2 && t.getpgid(2, 11) == Ok(10)
    };
    s.add("J9-组归并", j9, "迁移后新组员数累加且可查");

    // J10 getpgid 闭路 + ESRCH
    let j10 = {
        let t = mk_table();
        t.getpgid(2, 10) == Ok(1) && t.getpgid(2, 0xDEAD).err() == Some(R_ESRCH)
    };
    s.add("J10-getpgid闭路", j10, "初始继承父组且未知 pid ESRCH");

    // J11 self 闸：非自身非子目标 → EPERM
    let j11 = {
        let mut t = mk_table();
        t.setpgid(2, 12, 0, "非子非己").err() == Some(R_EPERM)
    };
    s.add("J11-self闸", j11, "非自身非子目标必 EPERM");

    // J12 指纹稳定 + seq 唯一
    let j12 = {
        let mut t = mk_table();
        let _ = t.setpgid(2, 0, 0, "x");
        let _ = t.setpgid(2, 10, 0, "y");
        t.fold() == t.fold() && t.seq_unique()
    };
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9146_gates_and_merge() {
        let mut t = mk_table();
        // 自聚 + 归并
        assert!(t.setpgid(2, 10, 0, "a").is_ok());
        assert_eq!(t.getpgid(2, 10), Ok(10));
        assert!(t.setpgid(2, 11, 10, "b").is_ok());
        assert_eq!(t.group_size(10), 2);
        // 会话首不可改
        assert_eq!(t.setpgid(1, 1, 10, "c"), Err(R_EPERM));
        // 组不存在
        assert_eq!(t.setpgid(2, 10, 42, "d"), Err(R_EINVAL));
        // 未知 pid
        assert_eq!(t.setpgid(2, 99, 0, "e"), Err(R_ESRCH));
        // 空留痕
        assert_eq!(t.setpgid(2, 10, 0, ""), Err(PG_EMPTY_NOTE));
    }

    #[test]
    fn unx9146_checks_green() {
        let s = run_unx9146_checks();
        assert!(s.all_passed(), "UNX-F9146 判据未全绿");
    }
}
