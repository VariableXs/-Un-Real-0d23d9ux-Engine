//! UNX-F10190 setsid 正反双判据（新会话达成与组首误用受控）（0x10ae0 段）
//!
//! 锚点：UNX-F10190-J1 新会话达成（setsid 后 pid=pgid=sid 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：会话册（pid × pgid × sid）+ setsid 核（组首必拒 EPERM/
//! 成功三号归一）+ 回读核（getpgid/getsid）+ 会话账本 + 折叠指纹 +
//! 注入核（组首调用/幽灵 pid/三号不齐伪造账/空留痕/印后调用）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10ae0 独占 0x20）。
pub const SEG_BASE: u32 = 0x10ae0;
pub const SD_GHOST: u32 = SEG_BASE + 0x01; // 幽灵 pid
pub const SD_EPERM: u32 = SEG_BASE + 0x02; // 组首调用被拒
pub const SD_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 留痕为空
pub const SD_FORGED: u32 = SEG_BASE + 0x04; // 三号不齐伪造账
pub const SD_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍调用

/// 进程条目：pid × pgid × sid（sid=0 表示尚无会话）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SidEnt {
    pub pid: u32,
    pub pgid: u32,
    pub sid: u32,
}

/// 一次 setsid 的账本留痕。
#[derive(Clone)]
pub struct SidRec {
    pub seq: u32,
    pub pid: u32,
    pub old_pgid: u32,
    pub new_sid: u32,
    pub note: String,
}

impl SidRec {
    /// 账目合法性：留痕非空且新会话 id==pid（setsid 语义）。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.new_sid == self.pid
    }

    pub fn render_line(&self) -> String {
        format!("D{}|p{}|g{}→s{}|{}", self.seq, self.pid, self.old_pgid, self.new_sid, self.note)
    }
}

/// 会话册：进程表 + setsid 核。
pub struct SidTab {
    pub procs: Vec<SidEnt>,
    pub ledger: Vec<SidRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SidTab {
    pub fn new() -> SidTab {
        SidTab { procs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记进程：初始同组（pgid=pid，无会话）。
    pub fn admit(&mut self, pid: u32, pgid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SD_FROZEN);
        }
        if pid == 0 || self.procs.iter().any(|p| p.pid == pid) {
            return Err(SD_GHOST);
        }
        self.procs.push(SidEnt { pid, pgid, sid: 0 });
        Ok(())
    }

    /// 组首判定：pid 是否为某在册进程的组首（pgid==pid 即自身为首）。
    pub fn is_leader(&self, pid: u32) -> bool {
        self.procs.iter().any(|p| p.pid == pid && p.pgid == pid)
    }

    /// setsid 核：组首必拒；成功则 pgid=sid=pid 三号归一。
    pub fn setsid(&mut self, pid: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(SD_FROZEN);
        }
        if note.is_empty() {
            return Err(SD_EMPTY_NOTE);
        }
        let i = match self.procs.iter().position(|p| p.pid == pid) {
            Some(i) => i,
            None => return Err(SD_GHOST),
        };
        if self.is_leader(pid) {
            return Err(SD_EPERM);
        }
        let old = self.procs[i].pgid;
        self.procs[i].pgid = pid;
        self.procs[i].sid = pid;
        self.ledger.push(SidRec {
            seq: self.seq_next,
            pid,
            old_pgid: old,
            new_sid: pid,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(pid)
    }

    /// 三号归一核验：pid==pgid==sid。
    pub fn triple_equal(&self, pid: u32) -> bool {
        match self.procs.iter().find(|p| p.pid == pid) {
            Some(p) => p.pid == p.pgid && p.pgid == p.sid && p.sid != 0,
            None => false,
        }
    }

    /// 会话清点：sid 会话下成员数（含首领）。
    pub fn session_size(&self, sid: u32) -> usize {
        self.procs.iter().filter(|p| p.sid == sid && sid != 0).count()
    }

    /// 会话首领数：全册中已立会话的进程数。
    pub fn sessions_total(&self) -> usize {
        self.procs.iter().filter(|p| p.sid != 0).count()
    }

    pub fn getpgid(&self, pid: u32) -> Result<u32, u32> {
        self.procs.iter().find(|p| p.pid == pid).map(|p| p.pgid).ok_or(SD_GHOST)
    }

    pub fn getsid(&self, pid: u32) -> Result<u32, u32> {
        self.procs.iter().find(|p| p.pid == pid).map(|p| p.sid).ok_or(SD_GHOST)
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

    /// 折叠指纹（127 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10ae0;
        for p in &self.procs {
            h = h.wrapping_mul(127).wrapping_add(p.pid);
            h = h.wrapping_mul(127).wrapping_add(p.pgid);
            h = h.wrapping_mul(127).wrapping_add(p.sid);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(127).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(127).wrapping_add(by as u32);
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
    let mut t = SidTab::new();
    let _ = t.admit(1, 1); // 组首
    let _ = t.admit(2, 1); // 组员
    // 1) 组首调用必拒（EPERM）。
    got[0] = t.setsid(1, "inj-leader").err() == Some(SD_EPERM);
    // 2) 幽灵 pid 必拒。
    got[1] = t.setsid(99, "inj-ghost").err() == Some(SD_GHOST);
    // 3) 三号不齐伪造账必判非法。
    let forged = SidRec {
        seq: 1,
        pid: 2,
        old_pgid: 1,
        new_sid: 7, // != pid
        note: "inj-uneq".to_string(),
    };
    got[2] = !forged.ok();
    // 4) 成功后三号归一（真实路径核验，负样本：入组前不齐）。
    let _ = t.setsid(2, "real");
    got[3] = t.triple_equal(2) && !t.triple_equal(1);
    // 5) 印账后调用必抓。
    t.seal();
    got[4] = t.setsid(2, "inj-frozen").err() == Some(SD_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10190_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10190");

    // J1 新会话达成：setsid 后 pid=pgid=sid。
    let j1 = {
        let mut t = SidTab::new();
        let _ = t.admit(1, 1);
        let _ = t.admit(2, 1);
        let r = t.setsid(2, "s");
        r == Ok(2) && t.triple_equal(2)
    };
    s.add("J1-三号归一", j1, "pid=pgid=sid 全等成立");

    // J2 组首误用受控：EPERM 且原组不漂。
    let j2 = {
        let mut t = SidTab::new();
        let _ = t.admit(1, 1);
        let e = t.setsid(1, "s");
        e == Err(SD_EPERM) && t.getpgid(1) == Ok(1)
    };
    s.add("J2-组首拒", j2, "组首 setsid 必拒且组号不动");

    // J3 回读双核：getpgid/getsid 一致。
    let j3 = {
        let mut t = SidTab::new();
        let _ = t.admit(1, 1);
        let _ = t.admit(2, 1);
        let _ = t.setsid(2, "s");
        t.getpgid(2) == Ok(2) && t.getsid(2) == Ok(2)
    };
    s.add("J3-回读双核", j3, "getpgid/getsid 双口一致");

    // J4 旧组脱离：组员走后旧组清点不含。
    let j4 = {
        let mut t = SidTab::new();
        let _ = t.admit(1, 1);
        let _ = t.admit(2, 1);
        let _ = t.admit(3, 1);
        let _ = t.setsid(2, "s");
        // 组 1 现仅剩组首 1 与组员 3；2 已自立新组。
        t.getpgid(3) == Ok(1) && t.getpgid(2) == Ok(2)
    };
    s.add("J4-旧组脱离", j4, "离组成员 pgid 自立");

    // J5 幽灵拒。
    let j5 = {
        let mut t = SidTab::new();
        t.setsid(9, "g").err() == Some(SD_GHOST)
    };
    s.add("J5-幽灵拒", j5, "未注册 pid 必拒");

    // J6 幽灵回读拒。
    let j6 = {
        let mut t = SidTab::new();
        t.getsid(9).err() == Some(SD_GHOST)
    };
    s.add("J6-幽灵回读拒", j6, "getsid 未注册 pid 必拒");

    // J7 账本完整：setsid 账全过合法性。
    let j7 = {
        let mut t = SidTab::new();
        let _ = t.admit(1, 1);
        let _ = t.admit(2, 1);
        let _ = t.setsid(2, "a");
        t.ledger.len() == 1 && t.ledger_ok() && t.seq_unique()
    };
    s.add("J7-账本完整", j7, "setsid 账合法无重号");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut t = SidTab::new();
        let _ = t.admit(1, 1);
        let _ = t.admit(2, 1);
        let _ = t.setsid(2, "s");
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2 && t.setsid(2, "post").err() == Some(SD_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // J10 会话清点：多人立会后 sessions_total 与逐会话规模对平。
    let j10 = {
        let mut t = SidTab::new();
        let _ = t.admit(1, 1);
        let _ = t.admit(2, 1);
        let _ = t.admit(3, 1);
        let _ = t.setsid(2, "a");
        let _ = t.setsid(3, "b");
        t.sessions_total() == 2 && t.session_size(2) == 1 && t.session_size(3) == 1
    };
    s.add("J10-会话清点", j10, "两会话各一员对平");

    // J11 连环立会：组长再立会按新组首语义受控。
    let j11 = {
        let mut t = SidTab::new();
        let _ = t.admit(1, 1);
        let _ = t.admit(2, 1);
        let _ = t.setsid(2, "s1");
        // 2 现为自身组首（pgid==pid），再立会必拒。
        t.setsid(2, "s2").err() == Some(SD_EPERM)
    };
    s.add("J11-连环立会拒", j11, "已立会者再立必拒");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10190_setsid_flow() {
        let mut t = SidTab::new();
        let _ = t.admit(1, 1);
        let _ = t.admit(2, 1);
        assert_eq!(t.setsid(1, "a"), Err(SD_EPERM));
        assert_eq!(t.setsid(2, "b"), Ok(2));
        assert!(t.triple_equal(2));
        assert!(!t.is_leader(2) || t.getpgid(2) == Ok(2));
    }

    #[test]
    fn unx10190_checks_green() {
        let s = run_unx10190_checks();
        assert!(s.all_passed(), "UNX-F10190 判据未全绿");
    }
}
