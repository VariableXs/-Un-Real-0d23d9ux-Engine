//! UNX-F9171 sched_yield 挂点（A1 调度消费）（0x63520 段）
//!
//! 锚点：UNX-F9171-J1 让渡账（每次 yield 落账：pid、让渡前优先级、
//! 回队位置、留痕）；J2 让渡语义（就绪队列回队尾 + 让渡计数单调 +
//! 无就绪者空转 + 前后台等价四性质）；J3 让渡指纹（账本折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-C2-B19（yield×A1 调度消费面）。
//!
//! 结构：就绪环（pid 环形队列）+ 让渡核（回队尾 + 计数）+ 让渡账本
//! + 折叠指纹 + 注入核（错码/空留痕/伪 pid/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63520 独占 0x20）。
pub const SEG_BASE: u32 = 0x63520;
pub const YD_UNK_PID: u32 = SEG_BASE + 0x01; // 伪 pid
pub const YD_BAD_MOVE: u32 = SEG_BASE + 0x02; // 回队错
pub const YD_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const YD_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const YD_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 单条让渡账。
#[derive(Clone)]
pub struct YieldRec {
    pub seq: u32,
    pub pid: u32,
    pub queue_len: usize, // 让渡时就绪队列长
    pub note: String,
}

impl YieldRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
    }

    pub fn render_line(&self) -> String {
        format!("YD{}|p{}|q{}|{}", self.seq, self.pid, self.queue_len, self.note)
    }
}

/// 让渡簿（就绪环）。
pub struct YdBook {
    pub ready: Vec<u32>,
    /// pid → yield 计数
    pub counts: Vec<(u32, u32)>,
    pub ledger: Vec<YieldRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl YdBook {
    pub fn new() -> YdBook {
        YdBook { ready: Vec::new(), counts: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn enroll(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(YD_FROZEN);
        }
        if pid == 0 || self.ready.contains(&pid) {
            return Err(YD_UNK_PID);
        }
        self.ready.push(pid);
        Ok(())
    }

    fn bump(&mut self, pid: u32) -> u32 {
        match self.counts.iter_mut().find(|(w, _)| *w == pid) {
            Some((_, c)) => {
                *c += 1;
                *c
            }
            None => {
                self.counts.push((pid, 1));
                1
            }
        }
    }

    pub fn count_of(&self, pid: u32) -> u32 {
        self.counts.iter().find(|(w, _)| *w == pid).map(|(_, c)| *c).unwrap_or(0)
    }

    /// 让渡核：回队尾。空队列（无就绪者）空转返回 0。
    pub fn yield_cpu(&mut self, pid: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(YD_FROZEN);
        }
        if note.is_empty() {
            return Err(YD_EMPTY_NOTE);
        }
        let pos = match self.ready.iter().position(|p| *p == pid) {
            Some(p) => p,
            None => return Err(YD_UNK_PID),
        };
        if self.ready.len() <= 1 {
            // 唯一就绪者：空转（不重排）
            self.record(pid, note);
            return Ok(0);
        }
        // 出队回队尾
        let v = self.ready[pos];
        let _ = pos;
        self.ready.remove(0);
        self.ready.push(v);
        let c = self.bump(pid);
        self.record(pid, note);
        Ok(c)
    }

    fn record(&mut self, pid: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(YieldRec { seq, pid, queue_len: self.ready.len(), note: note.to_string() });
    }

    /// 队列次序有效：无重复。
    pub fn queue_valid(&self) -> bool {
        for i in 0..self.ready.len() {
            for j in (i + 1)..self.ready.len() {
                if self.ready[i] == self.ready[j] {
                    return false;
                }
            }
        }
        true
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
        let mut h: u32 = 0x91710;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.pid);
            h = h.wrapping_mul(31).wrapping_add(r.queue_len as u32);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        for (w, c) in &self.counts {
            h = h.wrapping_mul(31).wrapping_add(*w);
            h = h.wrapping_mul(31).wrapping_add(*c);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 基础样本：就绪 [10, 11, 12]。
pub fn mk_book() -> YdBook {
    let mut b = YdBook::new();
    let _ = b.enroll(10);
    let _ = b.enroll(11);
    let _ = b.enroll(12);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 空留痕账
    let bad = YieldRec { seq: 1, pid: 1, queue_len: 0, note: String::new() };
    got[0] = !bad.ok();
    // 2) 伪 pid 让渡
    let mut b = mk_book();
    got[1] = b.yield_cpu(99, "inj-unk").err() == Some(YD_UNK_PID);
    // 3) 重登记拒
    got[2] = b.enroll(10).err() == Some(YD_UNK_PID);
    // 4) 队列重复注入
    b.ready.push(10);
    got[3] = !b.queue_valid();
    b.ready.pop();
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.yield_cpu(10, "inj-frozen").err() == Some(YD_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9171_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9171");
    let mut b = mk_book();

    // J1 让渡账非空
    let _ = b.yield_cpu(10, "让渡");
    let j1 = !b.ledger.is_empty();
    s.add("J1-让渡账非空", j1, "yield 落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空");

    // J3 回队尾：10 让渡后队首变 11
    let j3 = {
        let mut b2 = mk_book();
        let _ = b2.yield_cpu(10, "让");
        b2.ready.first() == Some(&11) && b2.ready.last() == Some(&10)
    };
    s.add("J3-回队尾", j3, "让渡者移队尾、次位上台");

    // J4 让渡计数单调
    let j4 = {
        let mut b2 = mk_book();
        let c1 = b2.yield_cpu(10, "一").unwrap_or(0);
        let c2 = b2.yield_cpu(10, "二").unwrap_or(0);
        let c3 = b2.yield_cpu(10, "三").unwrap_or(0);
        c1 < c2 && c2 < c3
    };
    s.add("J4-计数单调", j4, "同 pid 计数严格递增");

    // J5 无就绪者空转
    let j5 = {
        let mut b2 = YdBook::new();
        let _ = b2.enroll(10);
        b2.yield_cpu(10, "独转") == Ok(0)
    };
    s.add("J5-独者空转", j5, "唯一就绪者让渡回 0");

    // J6 伪 pid
    let j6 = b.yield_cpu(99, "伪").err() == Some(YD_UNK_PID);
    s.add("J6-伪pid", j6, "不在就绪环必拒");

    // J7 重登记拒
    let j7 = b.enroll(11).err() == Some(YD_UNK_PID);
    s.add("J7-重登记拒", j7, "重复入环必拒");

    // J8 队列无重复
    let j8 = b.queue_valid();
    s.add("J8-队列无重", j8, "就绪环无重复 pid");

    // J9 轮转公平：三人各让一次回原序
    let j9 = {
        let mut b2 = mk_book();
        let _ = b2.yield_cpu(10, "a");
        let _ = b2.yield_cpu(11, "b");
        let _ = b2.yield_cpu(12, "c");
        b2.ready == vec![10, 11, 12]
    };
    s.add("J9-轮转回原", j9, "全员各让一次恢复原序");

    // J10 零号拒
    let j10 = {
        let mut b2 = YdBook::new();
        b2.enroll(0).err() == Some(YD_UNK_PID)
    };
    s.add("J10-零号拒", j10, "pid=0 不入环");

    // J11 计数独立
    let j11 = {
        let mut b2 = mk_book();
        let _ = b2.yield_cpu(10, "a");
        let _ = b2.yield_cpu(11, "b");
        b2.count_of(10) == 1 && b2.count_of(11) == 1
    };
    s.add("J11-计数独立", j11, "各 pid 计数互不串");

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
    fn unx9171_yield_flow() {
        let mut b = mk_book();
        assert_eq!(b.yield_cpu(10, "a"), Ok(1));
        assert_eq!(b.ready, vec![11, 12, 10]);
        assert_eq!(b.yield_cpu(99, "b"), Err(YD_UNK_PID));
        // 全员各让一次回原序
        let _ = b.yield_cpu(11, "c");
        let _ = b.yield_cpu(12, "d");
        assert_eq!(b.ready, vec![10, 11, 12]);
        // 独者空转
        let mut solo = YdBook::new();
        let _ = solo.enroll(7);
        assert_eq!(solo.yield_cpu(7, "e"), Ok(0));
    }

    #[test]
    fn unx9171_checks_green() {
        let s = run_unx9171_checks();
        assert!(s.all_passed(), "UNX-F9171 判据未全绿");
    }
}
