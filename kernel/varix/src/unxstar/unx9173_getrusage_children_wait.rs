//! UNX-F9173 getrusage CHILDREN 联动（wait 收割累计）（0x63560 段）
//!
//! 锚点：UNX-F9173-J1 用量账（RUSAGE_SELF/CHILDREN 两档快照落账：
//! who、ru_utime/ru_stime/ru_maxrss、留痕）；J2 用量语义（SELF 自账
//! 不含子 + CHILDREN 只并孙不并己 + maxrss 取峰不累加 + 收割触发四
//! 性质）；J3 用量指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（getrusage 收割联动面）。
//!
//! 结构：用量册（pid × self/children 三元组）+ 消费核（burn 自耗 /
//! reap 并账）+ 用量账本 + 折叠指纹 + 注入核（错 who/空留痕/回退/
//! 重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63560 独占 0x20）。
pub const SEG_BASE: u32 = 0x63560;
pub const RU_UNK_WHO: u32 = SEG_BASE + 0x01; // 错 who
pub const RU_REGRESS: u32 = SEG_BASE + 0x02; // 用量回退
pub const RU_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const RU_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const RU_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// who 闭集。
pub const W_SELF: i32 = 0;
pub const W_CHILDREN: i32 = -1;

/// 用量三元组。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rusage {
    pub utime: u32,
    pub stime: u32,
    pub maxrss: u32,
}

impl Rusage {
    pub fn zero() -> Rusage {
        Rusage { utime: 0, stime: 0, maxrss: 0 }
    }

    /// 并账：时间累加，maxrss 取峰。
    pub fn merge(&mut self, o: &Rusage) {
        self.utime += o.utime;
        self.stime += o.stime;
        if o.maxrss > self.maxrss {
            self.maxrss = o.maxrss;
        }
    }

    pub fn render(&self) -> String {
        format!("u{} s{} rss{}", self.utime, self.stime, self.maxrss)
    }
}

/// 单条用量账。
#[derive(Clone)]
pub struct RuRec {
    pub seq: u32,
    pub pid: u32,
    pub who: i32,
    pub snap: Rusage,
    pub note: String,
}

impl RuRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.who == W_SELF || self.who == W_CHILDREN)
    }

    pub fn render_line(&self) -> String {
        format!(
            "RU{}|p{}|w{}|{}|{}",
            self.seq,
            self.pid,
            self.who,
            self.snap.render(),
            self.note
        )
    }
}

/// 用量册。
pub struct RuBook {
    /// (pid, who) → 三元组
    pub usage: Vec<((u32, i32), Rusage)>,
    pub ledger: Vec<RuRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl RuBook {
    pub fn new() -> RuBook {
        RuBook { usage: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    fn entry(&mut self, pid: u32, who: i32) -> usize {
        let key = (pid, who);
        if let Some(i) = self.usage.iter().position(|(k, _)| *k == key) {
            return i;
        }
        self.usage.push((key, Rusage::zero()));
        self.usage.len() - 1
    }

    /// 自耗记账：SELF 档。
    pub fn burn(&mut self, pid: u32, u: u32, st: u32, rss: u32, note: &str) -> Result<Rusage, u32> {
        if self.sealed {
            return Err(RU_FROZEN);
        }
        if note.is_empty() {
            return Err(RU_EMPTY_NOTE);
        }
        if pid == 0 {
            return Err(RU_REGRESS);
        }
        let i = self.entry(pid, W_SELF);
        let prev = self.usage[i].1;
        self.usage[i].1.utime += u;
        self.usage[i].1.stime += st;
        if rss > self.usage[i].1.maxrss {
            self.usage[i].1.maxrss = rss;
        }
        let cur = self.usage[i].1;
        if cur.utime < prev.utime || cur.stime < prev.stime {
            return Err(RU_REGRESS);
        }
        self.ledger.push(RuRec {
            seq: self.seq_next,
            pid,
            who: W_SELF,
            snap: cur,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(cur)
    }

    /// 收割并账：子 SELF 全额并入父 CHILDREN；maxrss 取峰不累加。
    pub fn reap_into(&mut self, parent: u32, child: u32, note: &str) -> Result<Rusage, u32> {
        if self.sealed {
            return Err(RU_FROZEN);
        }
        if note.is_empty() {
            return Err(RU_EMPTY_NOTE);
        }
        let ci = self.entry(child, W_SELF);
        let cu = self.usage[ci].1;
        let pi = self.entry(parent, W_CHILDREN);
        let prev = self.usage[pi].1;
        self.usage[pi].1.merge(&cu);
        let cur = self.usage[pi].1;
        if cur.utime < prev.utime || cur.stime < prev.stime {
            return Err(RU_REGRESS);
        }
        self.ledger.push(RuRec {
            seq: self.seq_next,
            pid: parent,
            who: W_CHILDREN,
            snap: cur,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(cur)
    }

    /// 快照。
    pub fn getrusage(&mut self, pid: u32, who: i32, note: &str) -> Result<Rusage, u32> {
        if self.sealed {
            return Err(RU_FROZEN);
        }
        if note.is_empty() {
            return Err(RU_EMPTY_NOTE);
        }
        if who != W_SELF && who != W_CHILDREN {
            return Err(RU_UNK_WHO);
        }
        let i = self.entry(pid, who);
        let cur = self.usage[i].1;
        self.ledger.push(RuRec { seq: self.seq_next, pid, who, snap: cur, note: note.to_string() });
        self.seq_next += 1;
        Ok(cur)
    }

    pub fn of(&self, pid: u32, who: i32) -> Option<Rusage> {
        self.usage.iter().find(|(k, _)| *k == (pid, who)).map(|(_, v)| *v)
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
        let mut h: u32 = 0x91730;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.pid);
            h = h.wrapping_mul(31).wrapping_add(r.snap.utime);
            h = h.wrapping_mul(31).wrapping_add(r.snap.stime);
            h = h.wrapping_mul(31).wrapping_add(r.snap.maxrss);
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
    // 1) 错 who
    let bad = RuRec {
        seq: 1,
        pid: 1,
        who: 9,
        snap: Rusage::zero(),
        note: "inj-who".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = RuRec {
        seq: 2,
        pid: 1,
        who: W_SELF,
        snap: Rusage::zero(),
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 错 who 查询
    let mut b = RuBook::new();
    got[2] = b.getrusage(1, 7, "inj-who").err() == Some(RU_UNK_WHO);
    // 4) 零号记账
    got[3] = b.burn(0, 1, 1, 1, "inj-zero").err() == Some(RU_REGRESS);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.burn(5, 1, 1, 1, "inj-frozen").err() == Some(RU_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9173_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9173");
    let mut b = RuBook::new();

    // J1 用量账非空（两档齐）
    let j1 = {
        let _ = b.burn(100, 8, 2, 512, "自耗");
        let _ = b.reap_into(100, 200, "收割");
        b.ledger.iter().filter(|r| r.who == W_SELF).count() >= 1
            && b.ledger.iter().filter(|r| r.who == W_CHILDREN).count() >= 1
    };
    s.add("J1-两档账齐", j1, "SELF/CHILDREN 各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 who 闭集");

    // J3 SELF 自账不含子
    let j3 = {
        let mut b2 = RuBook::new();
        let _ = b2.burn(100, 8, 2, 512, "自耗");
        let r = b2.of(100, W_SELF).unwrap_or(Rusage::zero());
        r.utime == 8 && r.stime == 2
    };
    s.add("J3-SELF自账", j3, "SELF 只含自身消耗");

    // J4 CHILDREN 只并子不并己
    let j4 = {
        let mut b2 = RuBook::new();
        let _ = b2.burn(100, 8, 2, 512, "父自耗");
        let _ = b2.burn(200, 10, 5, 1024, "子耗");
        let r = b2.reap_into(100, 200, "收割");
        r == Ok(Rusage { utime: 10, stime: 5, maxrss: 1024 })
    };
    s.add("J4-CHILDREN并子", j4, "CHILDREN 只含子孙账");

    // J5 maxrss 取峰不累加
    let j5 = {
        let mut b2 = RuBook::new();
        let _ = b2.burn(200, 1, 0, 2048, "子一峰");
        let _ = b2.burn(201, 1, 0, 512, "子二低");
        let r = b2.reap_into(100, 200, "收一");
        let _ = r;
        let r2 = b2.reap_into(100, 201, "收二");
        r2 == Ok(Rusage { utime: 2, stime: 0, maxrss: 2048 })
    };
    s.add("J5-maxrss取峰", j5, "双峰并账取最大不累加");

    // J6 时间累加
    let j6 = {
        let mut b2 = RuBook::new();
        let _ = b2.burn(200, 10, 0, 0, "子一");
        let _ = b2.burn(201, 20, 0, 0, "子二");
        let r = b2.reap_into(100, 200, "收一");
        let _ = r;
        b2.reap_into(100, 201, "收二") == Ok(Rusage { utime: 30, stime: 0, maxrss: 0 })
    };
    s.add("J6-时间累加", j6, "双子收割 utime 累加 30");

    // J7 错 who 拒
    let j7 = b.getrusage(100, 7, "野who").err() == Some(RU_UNK_WHO);
    s.add("J7-who闭集", j7, "who 出闭集必拒");

    // J8 零号记账拒
    let j8 = b.burn(0, 1, 1, 1, "零号").err() == Some(RU_REGRESS);
    s.add("J8-零号拒", j8, "pid=0 记账必拒");

    // J9 SELF 与 CHILDREN 独立
    let j9 = {
        let mut b2 = RuBook::new();
        let _ = b2.burn(100, 5, 5, 0, "自");
        let _ = b2.burn(200, 50, 50, 0, "子");
        let _ = b2.reap_into(100, 200, "收");
        let se = b2.of(100, W_SELF).unwrap_or(Rusage::zero());
        let ch = b2.of(100, W_CHILDREN).unwrap_or(Rusage::zero());
        se.utime == 5 && ch.utime == 50
    };
    s.add("J9-两档独立", j9, "SELF 不被收割污染");

    // J10 快照无副作用
    let j10 = {
        let mut b2 = RuBook::new();
        let _ = b2.burn(100, 5, 5, 100, "a");
        let before = b2.of(100, W_SELF);
        let _ = b2.getrusage(100, W_SELF, "快照");
        before == b2.of(100, W_SELF)
    };
    s.add("J10-快照无副作用", j10, "getrusage 不改账");

    // J11 空档零值
    let j11 = {
        let mut b2 = RuBook::new();
        b2.getrusage(300, W_CHILDREN, "空") == Ok(Rusage::zero())
    };
    s.add("J11-空档零值", j11, "无子进程 CHILDREN 全零");

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
    fn unx9173_rusage_flow() {
        let mut b = RuBook::new();
        let _ = b.burn(100, 5, 5, 100, "a");
        let _ = b.burn(200, 10, 2, 300, "b");
        let ch = b.reap_into(100, 200, "c").unwrap();
        assert_eq!(ch.utime, 10);
        assert_eq!(ch.maxrss, 300);
        let se = b.of(100, W_SELF).unwrap();
        assert_eq!(se.utime, 5);
        assert_eq!(b.getrusage(100, 7, "d"), Err(RU_UNK_WHO));
    }

    #[test]
    fn unx9173_checks_green() {
        let s = run_unx9173_checks();
        assert!(s.all_passed(), "UNX-F9173 判据未全绿");
    }
}
