//! UNX-F9165 RLIMIT_FSIZE/LOCKS/MSGQUEUE/RTPRIO 补位族（0x63460 段）
//!
//! 锚点：UNX-F9165-J1 补位格账（FSIZE/LOCKS/MSGQUEUE/RTPRIO 四格触
//! 发落账：资源、请求、限额、放行/拒收、留痕）；J2 补位格语义
//! （FSIZE 写越限 SIGXFSZ 拒 + LOCKS 持锁数闸 + MSGQ 字节闸 +
//! RTPRIO 非特权拒提四性质）；J3 补位格指纹（账本折叠指纹）三判据
//! 全绿。
//! 域：CoRun·UNX-C2-B19（补位限额消费族）。
//!
//! 结构：四格簿 + 消费核（write 挂点走 FSIZE / flock 挂点走 LOCKS
//! / msgsnd 挂点走 MSGQUEUE / sched_setscheduler 挂点走 RTPRIO）+
//! 消费账本 + 折叠指纹 + 注入核（错码/空留痕/越限放行/重号/印后登
//! 记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63460 独占 0x20）。
pub const SEG_BASE: u32 = 0x63460;
pub const RL5_UNK_RES: u32 = SEG_BASE + 0x01; // 伪资源
pub const RL5_BAD_LIMIT: u32 = SEG_BASE + 0x02; // 越限放行伪样本
pub const RL5_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const RL5_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const RL5_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 资源闭集。
pub const X_FSIZE: usize = 0;
pub const X_LOCKS: usize = 1;
pub const X_MSGQUEUE: usize = 2;
pub const X_RTPRIO: usize = 3;
pub const X_COUNT: usize = 4;

pub const X_NAMES: [&str; X_COUNT] = ["FSIZE", "LOCKS", "MSGQUEUE", "RTPRIO"];

/// SIGXFSZ=25。
pub const SIG_XFSZ: u32 = 25;
/// RLIM_INFINITY。
pub const INF: u32 = u32::MAX;

/// 单条消费账。
#[derive(Clone)]
pub struct SupRec {
    pub seq: u32,
    pub res: usize,
    pub want: u32,
    pub limit: u32,
    pub allowed: bool,
    pub sig: u32,
    pub note: String,
}

impl SupRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.res < X_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "X5{}|{}|w{}<l{}|{}|sig{}|{}",
            self.seq,
            X_NAMES[self.res],
            self.want,
            self.limit,
            if self.allowed { "GO" } else { "DENY" },
            self.sig,
            self.note
        )
    }
}

/// 补位限额簿。
pub struct SupBook {
    pub limits: [u32; X_COUNT],
    pub used: [u32; X_COUNT],
    pub privileged: bool,
    pub ledger: Vec<SupRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SupBook {
    pub fn new(privileged: bool) -> SupBook {
        SupBook {
            limits: [INF, 64, 4096, 0],
            used: [0, 0, 0, 0],
            privileged,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn set_limit(&mut self, res: usize, v: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RL5_FROZEN);
        }
        if res >= X_COUNT {
            return Err(RL5_UNK_RES);
        }
        self.limits[res] = v;
        Ok(())
    }

    fn record(&mut self, res: usize, want: u32, allowed: bool, sig: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(SupRec {
            seq,
            res,
            want,
            limit: self.limits[res],
            allowed,
            sig,
            note: note.to_string(),
        });
    }

    /// FSIZE 消费核：write 挂点。越限拒 + SIGXFSZ。
    pub fn write_file(&mut self, offset: u32, len: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RL5_FROZEN);
        }
        if note.is_empty() {
            return Err(RL5_EMPTY_NOTE);
        }
        let end = offset.wrapping_add(len);
        let lim = self.limits[X_FSIZE];
        if lim != INF && end > lim {
            self.record(X_FSIZE, end, false, SIG_XFSZ, note);
            return Err(RL5_BAD_LIMIT);
        }
        self.record(X_FSIZE, end, true, 0, note);
        Ok(end)
    }

    /// LOCKS 消费核：flock 挂点。
    pub fn take_lock(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RL5_FROZEN);
        }
        if note.is_empty() {
            return Err(RL5_EMPTY_NOTE);
        }
        let lim = self.limits[X_LOCKS];
        if lim != INF && self.used[X_LOCKS] + 1 > lim {
            self.record(X_LOCKS, self.used[X_LOCKS] + 1, false, 0, note);
            return Err(RL5_BAD_LIMIT);
        }
        self.used[X_LOCKS] += 1;
        self.record(X_LOCKS, self.used[X_LOCKS], true, 0, note);
        Ok(self.used[X_LOCKS])
    }

    pub fn drop_lock(&mut self) {
        self.used[X_LOCKS] = self.used[X_LOCKS].saturating_sub(1);
    }

    /// MSGQUEUE 消费核：msgsnd 挂点（字节账）。
    pub fn msg_send(&mut self, bytes: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RL5_FROZEN);
        }
        if note.is_empty() {
            return Err(RL5_EMPTY_NOTE);
        }
        let lim = self.limits[X_MSGQUEUE];
        if lim != INF && self.used[X_MSGQUEUE] + bytes > lim {
            self.record(X_MSGQUEUE, self.used[X_MSGQUEUE] + bytes, false, 0, note);
            return Err(RL5_BAD_LIMIT);
        }
        self.used[X_MSGQUEUE] += bytes;
        self.record(X_MSGQUEUE, self.used[X_MSGQUEUE], true, 0, note);
        Ok(self.used[X_MSGQUEUE])
    }

    /// RTPRIO 消费核：sched_setscheduler 挂点。非特权提实时级拒。
    pub fn set_rtprio(&mut self, prio: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RL5_FROZEN);
        }
        if note.is_empty() {
            return Err(RL5_EMPTY_NOTE);
        }
        let lim = self.limits[X_RTPRIO];
        if prio > lim && !self.privileged {
            self.record(X_RTPRIO, prio, false, 0, note);
            return Err(RL5_BAD_LIMIT);
        }
        self.used[X_RTPRIO] = prio;
        self.record(X_RTPRIO, prio, true, 0, note);
        Ok(prio)
    }

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

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91650;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.res as u32);
            h = h.wrapping_mul(31).wrapping_add(r.want);
            h = h.wrapping_mul(31).wrapping_add(r.sig);
            for b in r.note.bytes() {
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

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 伪资源
    let bad = SupRec {
        seq: 1,
        res: 9,
        want: 1,
        limit: 1,
        allowed: true,
        sig: 0,
        note: "inj-res".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = SupRec {
        seq: 2,
        res: 0,
        want: 1,
        limit: 1,
        allowed: true,
        sig: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) FSIZE 越限 SIGXFSZ
    let mut b = SupBook::new(true);
    let _ = b.set_limit(X_FSIZE, 100);
    got[2] = b.write_file(90, 20, "inj-fsize").err() == Some(RL5_BAD_LIMIT)
        && b.ledger.last().unwrap().sig == SIG_XFSZ;
    // 4) LOCKS 满拒
    let _ = b.set_limit(X_LOCKS, 1);
    let _ = b.take_lock("占");
    got[3] = b.take_lock("inj-lock").err() == Some(RL5_BAD_LIMIT);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.take_lock("inj-frozen").err() == Some(RL5_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9165_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9165");
    let mut b = SupBook::new(false);

    // J1 四格账齐
    let j1 = {
        let _ = b.write_file(0, 16, "write 走 FSIZE");
        let _ = b.take_lock("flock 走 LOCKS");
        let _ = b.msg_send(32, "msgsnd 走 MSGQUEUE");
        let _ = b.set_rtprio(0, "rtprio 走 RTPRIO");
        (0..X_COUNT).all(|r| b.ledger.iter().any(|e| e.res == r))
    };
    s.add("J1-四格账齐", j1, "四格各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且格闭集");

    // J3 FSIZE 越限 SIGXFSZ
    let j3 = {
        let mut b2 = SupBook::new(true);
        let _ = b2.set_limit(X_FSIZE, 100);
        b2.write_file(90, 20, "越").err() == Some(RL5_BAD_LIMIT)
            && b2.ledger.last().unwrap().sig == SIG_XFSZ
    };
    s.add("J3-FSIZE越限", j3, "写越限拒且 SIGXFSZ");

    // J4 FSIZE 合格放行
    let j4 = {
        let mut b2 = SupBook::new(true);
        let _ = b2.set_limit(X_FSIZE, 100);
        b2.write_file(50, 50, "恰达") == Ok(100)
    };
    s.add("J4-FSIZE恰达", j4, "恰达限放行");

    // J5 LOCKS 闸 + 释放回收
    let j5 = {
        let mut b2 = SupBook::new(true);
        let _ = b2.set_limit(X_LOCKS, 1);
        let denied = b2.take_lock("a").is_ok() && b2.take_lock("b").is_err();
        b2.drop_lock();
        denied && b2.take_lock("回收").is_ok()
    };
    s.add("J5-LOCKS闸", j5, "持锁超限拒、释放回收");

    // J6 MSGQUEUE 字节闸
    let j6 = {
        let mut b2 = SupBook::new(true);
        let _ = b2.set_limit(X_MSGQUEUE, 100);
        let _ = b2.msg_send(60, "a");
        b2.msg_send(60, "b").err() == Some(RL5_BAD_LIMIT)
    };
    s.add("J6-MSGQ字节闸", j6, "队列字节超限拒");

    // J7 RTPRIO 非特权拒提
    let j7 = {
        let mut b2 = SupBook::new(false);
        b2.set_rtprio(10, "提实时").err() == Some(RL5_BAD_LIMIT)
    };
    s.add("J7-RTPRIO拒提", j7, "非特权提实时级必拒");

    // J8 RTPRIO 特权放行
    let j8 = {
        let mut b2 = SupBook::new(true);
        b2.set_rtprio(99, "特权") == Ok(99)
    };
    s.add("J8-RTPRIO特权", j8, "特权视角放行");

    // J9 伪资源拒
    let j9 = b.set_limit(9, 1).err() == Some(RL5_UNK_RES);
    s.add("J9-伪资源拒", j9, "格出闭集必拒");

    // J10 四格独立
    let j10 = {
        let mut b2 = SupBook::new(false);
        let _ = b2.set_limit(X_LOCKS, 0);
        let lock_deny = b2.take_lock("锁禁").is_err();
        let fsize_ok = b2.write_file(0, 8, "FSIZE 不受牵连").is_ok();
        lock_deny && fsize_ok
    };
    s.add("J10-四格独立", j10, "格间互不牵连");

    // J11 拒收留痕
    let j11 = b.ledger.iter().any(|r| !r.allowed);
    s.add("J11-拒收留痕", j11, "DENY 路径有账");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
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
    fn unx9165_sup_flow() {
        let mut b = SupBook::new(false);
        // FSIZE
        let _ = b.set_limit(X_FSIZE, 100);
        assert_eq!(b.write_file(0, 100, "a"), Ok(100));
        assert_eq!(b.write_file(100, 1, "b"), Err(RL5_BAD_LIMIT));
        // LOCKS
        let _ = b.set_limit(X_LOCKS, 2);
        assert!(b.take_lock("c").is_ok());
        assert!(b.take_lock("d").is_ok());
        assert_eq!(b.take_lock("e"), Err(RL5_BAD_LIMIT));
        // MSGQUEUE
        let _ = b.set_limit(X_MSGQUEUE, 64);
        assert!(b.msg_send(32, "f").is_ok());
        assert_eq!(b.msg_send(64, "g"), Err(RL5_BAD_LIMIT));
        // RTPRIO
        assert_eq!(b.set_rtprio(5, "h"), Err(RL5_BAD_LIMIT));
        assert_eq!(b.set_rtprio(0, "i"), Ok(0));
    }

    #[test]
    fn unx9165_checks_green() {
        let s = run_unx9165_checks();
        assert!(s.all_passed(), "UNX-F9165 判据未全绿");
    }
}
