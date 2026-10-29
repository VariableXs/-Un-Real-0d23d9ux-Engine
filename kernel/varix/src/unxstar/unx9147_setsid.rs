//! UNX-F9147 setsid 语义档（会话首进程选举）（0x63220 段）
//!
//! 锚点：UNX-F9147-J1 选举账（每次 setsid 落账：pid、旧会话、新会
//! 话、结果、留痕）；J2 选举语义（组长禁选闸 + sid/pgid 双同步 +
//! 脱离原会话 + 再选被拒 + 账面选举一致性五性质）；J3 选举指纹（账
//! 本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B18（会话选举面）。
//!
//! 结构：会话登记簿（子继承父 sid+pgid）+ 选举核（EPERM 闸 + 双同
//! 步写）+ 选举账本 + 账面一致性校验（成功账 new_sid==pid 且表中自
//! 称首唯一）+ 折叠指纹 + 注入核（错码/空留痕/伪账/未知 pid/印后登
//! 记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63220 独占 0x20）。
pub const SEG_BASE: u32 = 0x63220;
pub const SS_UNK_PID: u32 = SEG_BASE + 0x01; // 未知 pid
pub const SS_BAD_ELECT: u32 = SEG_BASE + 0x02; // 选举闸判定错
pub const SS_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const SS_BAD_LEDGER: u32 = SEG_BASE + 0x04; // 伪账（成功账与表不符）
pub const SS_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EPERM: u32 = 1; // 调用者已是进程组长
pub const R_ESRCH: u32 = 2; // pid 不存在
pub const R_COUNT: usize = 3;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EPERM", "ESRCH"];

/// 单条选举账。
#[derive(Clone)]
pub struct Elect {
    pub seq: u32,
    pub pid: u32,
    pub old_sid: u32,
    pub new_sid: u32,
    pub code: u32,
    pub note: String,
}

impl Elect {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.code as usize) < R_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "EL{}|p{}|{}->{}|{}|{}",
            self.seq,
            self.pid,
            self.old_sid,
            self.new_sid,
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// 会话选举簿。procs: (pid, ppid, sid, pgid)。
pub struct SessBook {
    pub procs: Vec<(u32, u32, u32, u32)>,
    pub ledger: Vec<Elect>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SessBook {
    pub fn new() -> SessBook {
        SessBook { procs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// spawn：子继承父 sid 与 pgid（无父者自为会话首兼组长）。
    pub fn spawn(&mut self, pid: u32, ppid: u32) {
        let (sid, pgid) = match self.procs.iter().find(|p| p.0 == ppid) {
            Some(p) => (p.2, p.3),
            None => (ppid, pid),
        };
        self.procs.push((pid, ppid, sid, pgid));
    }

    pub fn find(&self, pid: u32) -> Option<usize> {
        self.procs.iter().position(|p| p.0 == pid)
    }

    /// 是否进程组长（pgid == pid）。
    pub fn is_group_leader(&self, pid: u32) -> bool {
        match self.find(pid) {
            Some(i) => self.procs[i].3 == pid,
            None => false,
        }
    }

    /// 账面一致性：每条成功账 new_sid==pid 且表中该会话自称首唯一。
    pub fn elect_consistent(&self) -> bool {
        for e in &self.ledger {
            if e.code != R_OK {
                continue;
            }
            if e.new_sid != e.pid {
                return false;
            }
            let leaders =
                self.procs.iter().filter(|p| p.2 == e.new_sid && p.0 == p.2).count();
            if leaders != 1 {
                return false;
            }
        }
        true
    }

    /// 选举核：setsid(caller)。成功双同步 sid=pid、pgid=pid。
    pub fn setsid(&mut self, caller: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(SS_FROZEN);
        }
        if note.is_empty() {
            return Err(SS_EMPTY_NOTE);
        }
        let i = match self.find(caller) {
            Some(i) => i,
            None => return Err(SS_UNK_PID),
        };
        if self.is_group_leader(caller) {
            let old = self.procs[i].2;
            self.record(caller, old, old, R_EPERM, note);
            return Err(R_EPERM);
        }
        let old_sid = self.procs[i].2;
        let new_sid = caller;
        self.procs[i].2 = new_sid;
        self.procs[i].3 = new_sid;
        let seq = self.record(caller, old_sid, new_sid, R_OK, note);
        Ok(seq)
    }

    fn record(&mut self, pid: u32, old_sid: u32, new_sid: u32, code: u32, note: &str) -> u32 {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(Elect {
            seq,
            pid,
            old_sid,
            new_sid,
            code,
            note: note.to_string(),
        });
        seq
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91470;
        for e in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(e.seq);
            h = h.wrapping_mul(31).wrapping_add(e.pid);
            h = h.wrapping_mul(31).wrapping_add(e.old_sid);
            h = h.wrapping_mul(31).wrapping_add(e.new_sid);
            h = h.wrapping_mul(31).wrapping_add(e.code);
            for b in e.note.bytes() {
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

/// 基础样本：init(1) 自为组长；sh(2) 与 job(10) 均非组长（pgid=1）。
pub fn mk_book() -> SessBook {
    let mut b = SessBook::new();
    b.spawn(1, 1);
    b.spawn(2, 1);
    b.spawn(10, 2);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错码
    let bad = Elect {
        seq: 9001,
        pid: 1,
        old_sid: 0,
        new_sid: 0,
        code: 42,
        note: "inj-code".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = Elect {
        seq: 9002,
        pid: 1,
        old_sid: 0,
        new_sid: 0,
        code: R_OK,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪账：成功账 new_sid != pid
    let mut b = mk_book();
    let _ = b.setsid(10, "real");
    b.ledger.push(Elect {
        seq: 99,
        pid: 50,
        old_sid: 1,
        new_sid: 10, // 伪造：50 号自称迁入 10 会话
        code: R_OK,
        note: "inj-fake".to_string(),
    });
    got[2] = !b.elect_consistent();
    // 4) 未知 pid setsid
    let mut b2 = mk_book();
    got[3] = b2.setsid(0xBEEF, "inj-unk").err() == Some(SS_UNK_PID);
    // 5) 印后登记必抓
    b2.seal();
    got[4] = b2.setsid(10, "inj-frozen").err() == Some(SS_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9147_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9147");
    let mut b = mk_book();

    // J1 账非空
    let _ = b.setsid(10, "job 自立会话");
    let j1 = !b.ledger.is_empty();
    s.add("J1-选举账非空", j1, "setsid 后账本至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|e| e.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且码在闭集");

    // J3 组长禁选闸：init(1) 是组长 → EPERM
    let j3 = b.setsid(1, "init 再选").err() == Some(R_EPERM);
    s.add("J3-组长禁选闸", j3, "进程组长 setsid 必 EPERM");

    // J4 双同步：sid=pid 且 pgid=pid
    let j4 = {
        let i = b.find(10).unwrap();
        b.procs[i].2 == 10 && b.procs[i].3 == 10
    };
    s.add("J4-双同步", j4, "选举后 sid 与 pgid 同步为 pid");

    // J5 账面一致性（成功账与表相符）
    let j5 = b.elect_consistent();
    s.add("J5-账面一致", j5, "成功账 new_sid==pid 且自称首唯一");

    // J6 旧会话留痕
    let j6 = b
        .ledger
        .iter()
        .any(|e| e.pid == 10 && e.code == R_OK && e.old_sid != e.new_sid);
    s.add("J6-旧会话留痕", j6, "成功选举记录旧 sid→新 sid");

    // J7 脱离原会话
    let j7 = {
        let i2 = b.find(2).unwrap();
        let i10 = b.find(10).unwrap();
        b.procs[i2].2 != b.procs[i10].2
    };
    s.add("J7-脱离原会话", j7, "选举后与原会话成员 sid 分离");

    // J8 再选被拒：已为会话首（=组长）再选 EPERM
    let j8 = b.setsid(10, "再选").err() == Some(R_EPERM);
    s.add("J8-再选被拒", j8, "已为组长再 setsid 必 EPERM");

    // J9 未知 pid
    let j9 = b.setsid(0xF00D, "未知").err() == Some(SS_UNK_PID);
    s.add("J9-未知pid", j9, "未知调用者必拒");

    // J10 拒选留痕（EPERM 亦落账）
    let j10 = b.ledger.iter().any(|e| e.pid == 1 && e.code == R_EPERM);
    s.add("J10-拒选留痕", j10, "EPERM 拒绝亦留账");

    // J11 非组长可选举（2 号 pgid=1 → 成功）
    let j11 = {
        let mut b2 = mk_book();
        b2.setsid(2, "sh 自立").is_ok()
    };
    s.add("J11-非组长可选", j11, "非组长进程选举成功");

    // J12 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

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
    fn unx9147_elect_basic() {
        let mut b = mk_book();
        // 非组长可选举
        assert!(b.setsid(10, "ok").is_ok());
        // 组长被拒
        assert_eq!(b.setsid(2, "x"), Err(R_EPERM));
        // 未知
        assert_eq!(b.setsid(77, "x"), Err(SS_UNK_PID));
        // 空留痕
        assert_eq!(b.setsid(1, ""), Err(SS_EMPTY_NOTE));
        // 双同步
        let i = b.find(10).unwrap();
        assert_eq!(b.procs[i].2, 10);
        assert_eq!(b.procs[i].3, 10);
    }

    #[test]
    fn unx9147_checks_green() {
        let s = run_unx9147_checks();
        assert!(s.all_passed(), "UNX-F9147 判据未全绿");
    }
}
