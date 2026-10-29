//! UNX-F9164 RLIMIT_CPU/NPROC/SIGPENDING 调度限额族（0x63440 段）
//!
//! 锚点：UNX-F9164-J1 调度格账（CPU/NPROC/SIGPENDING 每格触发落
//! 账：资源、请求、限额、放行/拒收、留痕）；J2 调度格语义（CPU 超
//! 软限 SIGXCPU 告警 + NPROC fork 拒 + SIGPENDING 排队闸三性质）；
//! J3 调度格指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（调度限额消费族）。
//!
//! 结构：三格簿 + 消费核（tick 挂点走 CPU / fork 挂点走 NPROC /
//! sigqueue 挂点走 SIGPENDING）+ 消费账本 + 折叠指纹 + 注入核（错
//! 码/空留痕/越限放行/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63440 独占 0x20）。
pub const SEG_BASE: u32 = 0x63440;
pub const RL4_UNK_RES: u32 = SEG_BASE + 0x01; // 伪资源
pub const RL4_BAD_LIMIT: u32 = SEG_BASE + 0x02; // 越限放行伪样本
pub const RL4_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const RL4_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const RL4_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 资源闭集。
pub const S_CPU: usize = 0;
pub const S_NPROC: usize = 1;
pub const S_SIGPENDING: usize = 2;
pub const S_COUNT: usize = 3;

pub const S_NAMES: [&str; S_COUNT] = ["CPU", "NPROC", "SIGPENDING"];

/// 信号：SIGXCPU=24、SIGKILL=9。
pub const SIG_XCPU: u32 = 24;
pub const SIG_KILL: u32 = 9;

/// 单条消费账。
#[derive(Clone)]
pub struct SchedRec {
    pub seq: u32,
    pub res: usize,
    pub want: u32,
    pub limit: u32,
    pub allowed: bool,
    pub sig: u32, // 触发的信号（0=无）
    pub note: String,
}

impl SchedRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.res < S_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "S4{}|{}|w{}<l{}|{}|sig{}|{}",
            self.seq,
            S_NAMES[self.res],
            self.want,
            self.limit,
            if self.allowed { "GO" } else { "DENY" },
            self.sig,
            self.note
        )
    }
}

/// 调度限额簿。
pub struct SchBook {
    pub limits: [u32; S_COUNT],
    pub used: [u32; S_COUNT],
    /// CPU 软限已告警（SIGXCPU 只发一次）
    pub xcpu_sent: bool,
    pub ledger: Vec<SchedRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SchBook {
    pub fn new() -> SchBook {
        SchBook {
            limits: [10, 256, 64],
            used: [0, 2, 0],
            xcpu_sent: false,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn set_limit(&mut self, res: usize, v: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RL4_FROZEN);
        }
        if res >= S_COUNT {
            return Err(RL4_UNK_RES);
        }
        self.limits[res] = v;
        Ok(())
    }

    fn record(&mut self, res: usize, want: u32, allowed: bool, sig: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(SchedRec {
            seq,
            res,
            want,
            limit: self.limits[res],
            allowed,
            sig,
            note: note.to_string(),
        });
    }

    /// CPU 消费核：tick 挂点。达硬限 SIGKILL；达软限 SIGXCPU 一次。
    pub fn cpu_tick(&mut self, secs: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RL4_FROZEN);
        }
        if note.is_empty() {
            return Err(RL4_EMPTY_NOTE);
        }
        self.used[S_CPU] = secs;
        let lim = self.limits[S_CPU];
        if secs >= lim {
            self.record(S_CPU, secs, false, SIG_KILL, note);
            return Err(RL4_BAD_LIMIT);
        }
        // 软限 = 硬限的 90%（样本规则）
        let soft = lim * 9 / 10;
        if secs >= soft && !self.xcpu_sent {
            self.xcpu_sent = true;
            self.record(S_CPU, secs, true, SIG_XCPU, note);
            return Ok(secs);
        }
        self.record(S_CPU, secs, true, 0, note);
        Ok(secs)
    }

    /// NPROC 消费核：fork 挂点。
    pub fn fork_proc(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RL4_FROZEN);
        }
        if note.is_empty() {
            return Err(RL4_EMPTY_NOTE);
        }
        let lim = self.limits[S_NPROC];
        if self.used[S_NPROC] + 1 > lim {
            self.record(S_NPROC, self.used[S_NPROC] + 1, false, 0, note);
            return Err(RL4_BAD_LIMIT);
        }
        self.used[S_NPROC] += 1;
        self.record(S_NPROC, self.used[S_NPROC], true, 0, note);
        Ok(self.used[S_NPROC])
    }

    pub fn exit_proc(&mut self) {
        self.used[S_NPROC] = self.used[S_NPROC].saturating_sub(1);
    }

    /// SIGPENDING 消费核：sigqueue 挂点。
    pub fn sig_queue(&mut self, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RL4_FROZEN);
        }
        if note.is_empty() {
            return Err(RL4_EMPTY_NOTE);
        }
        let lim = self.limits[S_SIGPENDING];
        if self.used[S_SIGPENDING] + 1 > lim {
            self.record(S_SIGPENDING, self.used[S_SIGPENDING] + 1, false, 0, note);
            return Err(RL4_BAD_LIMIT);
        }
        self.used[S_SIGPENDING] += 1;
        self.record(S_SIGPENDING, self.used[S_SIGPENDING], true, 0, note);
        Ok(self.used[S_SIGPENDING])
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
        let mut h: u32 = 0x91640;
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
    let bad = SchedRec {
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
    let bad2 = SchedRec {
        seq: 2,
        res: 0,
        want: 1,
        limit: 1,
        allowed: true,
        sig: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) CPU 硬限 SIGKILL
    let mut b = SchBook::new();
    got[2] = b.cpu_tick(10, "inj-cpu").err() == Some(RL4_BAD_LIMIT);
    // 4) NPROC 满拒
    b.used[S_NPROC] = 256;
    got[3] = b.fork_proc("inj-nproc").err() == Some(RL4_BAD_LIMIT);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.fork_proc("inj-frozen").err() == Some(RL4_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9164_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9164");
    let mut b = SchBook::new();

    // J1 三格账齐
    let j1 = {
        let _ = b.cpu_tick(1, "tick 走 CPU");
        let _ = b.fork_proc("fork 走 NPROC");
        let _ = b.sig_queue("sigq 走 SIGPENDING");
        b.ledger.iter().filter(|r| r.res == S_CPU).count() >= 1
            && b.ledger.iter().filter(|r| r.res == S_NPROC).count() >= 1
            && b.ledger.iter().filter(|r| r.res == S_SIGPENDING).count() >= 1
    };
    s.add("J1-三格账齐", j1, "CPU/NPROC/SIGPENDING 各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且格闭集");

    // J3 CPU 软限 SIGXCPU 一次
    let j3 = {
        let mut b2 = SchBook::new();
        let _ = b2.set_limit(S_CPU, 10);
        let r1 = b2.cpu_tick(9, "达软");
        let r2 = b2.cpu_tick(9, "再达软");
        r1 == Ok(9) && b2.ledger.iter().any(|r| r.sig == SIG_XCPU)
            && r2 == Ok(9)
            && b2.ledger.iter().filter(|r| r.sig == SIG_XCPU).count() == 1
    };
    s.add("J3-SIGXCPU一次", j3, "软限告警恰一次");

    // J4 CPU 硬限 SIGKILL
    let j4 = {
        let mut b2 = SchBook::new();
        let _ = b2.set_limit(S_CPU, 10);
        b2.cpu_tick(10, "达硬").err() == Some(RL4_BAD_LIMIT)
            && b2.ledger.last().unwrap().sig == SIG_KILL
    };
    s.add("J4-硬限SIGKILL", j4, "达硬限拒且 SIGKILL");

    // J5 NPROC 满拒
    let j5 = {
        let mut b2 = SchBook::new();
        let _ = b2.set_limit(S_NPROC, 3);
        let _ = b2.fork_proc("a");
        let _ = b2.fork_proc("b");
        b2.fork_proc("满").err() == Some(RL4_BAD_LIMIT)
    };
    s.add("J5-NPROC满拒", j5, "进程数超限 fork 必拒");

    // J6 NPROC 退出回收
    let j6 = {
        let mut b2 = SchBook::new();
        let _ = b2.set_limit(S_NPROC, 3);
        let _ = b2.fork_proc("a");
        b2.exit_proc();
        b2.fork_proc("回收后").is_ok()
    };
    s.add("J6-exit回收", j6, "退出释放配额");

    // J7 SIGPENDING 满拒
    let j7 = {
        let mut b2 = SchBook::new();
        let _ = b2.set_limit(S_SIGPENDING, 2);
        let _ = b2.sig_queue("a");
        let _ = b2.sig_queue("b");
        b2.sig_queue("满").err() == Some(RL4_BAD_LIMIT)
    };
    s.add("J7-排队满拒", j7, "排队信号超限必拒");

    // J8 SIGPENDING 消费后可再排（简化：无 drain 挂点时以 limit 调整表达）
    let j8 = {
        let mut b2 = SchBook::new();
        let _ = b2.set_limit(S_SIGPENDING, 2);
        let _ = b2.sig_queue("a");
        let _ = b2.set_limit(S_SIGPENDING, 10);
        b2.sig_queue("扩容后").is_ok()
    };
    s.add("J8-扩容再排", j8, "限额上调后排队放行");

    // J9 伪资源拒
    let j9 = b.set_limit(9, 1).err() == Some(RL4_UNK_RES);
    s.add("J9-伪资源拒", j9, "格出闭集必拒");

    // J10 三格独立
    let j10 = {
        let mut b2 = SchBook::new();
        let _ = b2.set_limit(S_NPROC, 2);
        let _ = b2.fork_proc("a");
        let _ = b2.fork_proc("b");
        let nproc_full = b2.fork_proc("满").is_err();
        let cpu_ok = b2.cpu_tick(1, "CPU 不受牵连").is_ok();
        nproc_full && cpu_ok
    };
    s.add("J10-三格独立", j10, "格间互不牵连");

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
    fn unx9164_sched_flow() {
        let mut b = SchBook::new();
        let _ = b.set_limit(S_CPU, 10);
        assert_eq!(b.cpu_tick(5, "a"), Ok(5));
        assert_eq!(b.cpu_tick(9, "b"), Ok(9));
        assert!(b.ledger.iter().any(|r| r.sig == SIG_XCPU));
        assert_eq!(b.cpu_tick(10, "c"), Err(RL4_BAD_LIMIT));
        // NPROC
        let _ = b.set_limit(S_NPROC, 3);
        assert!(b.fork_proc("d").is_ok());
        b.exit_proc();
        assert!(b.fork_proc("e").is_ok());
        // SIGPENDING
        let _ = b.set_limit(S_SIGPENDING, 1);
        assert!(b.sig_queue("f").is_ok());
        assert_eq!(b.sig_queue("g"), Err(RL4_BAD_LIMIT));
    }

    #[test]
    fn unx9164_checks_green() {
        let s = run_unx9164_checks();
        assert!(s.all_passed(), "UNX-F9164 判据未全绿");
    }
}
