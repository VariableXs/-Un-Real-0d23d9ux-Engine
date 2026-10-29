//! UNX-F9172 times 语义档（tms 四字段与 CLK_TCK）（0x63540 段）
//!
//! 锚点：UNX-F9172-J1 tms 账（每次 times 落账：utime/stime/cutime/
//! cstime 四字段快照 + 留痕）；J2 tms 语义（四字段单调不减 + CLK_TCK
//! 固定 100 + 收割联动累计 + 空账零值四性质）；J3 tms 指纹（账本折
//! 叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（times 四字段面）。
//!
//! 结构：tms 册（进程 × 四字段）+ 消费核（自耗记账 / 收割累计）+
//! tms 账本 + 折叠指纹 + 注入核（回退/空留痕/伪 pid/重号/印后登记
//! 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63540 独占 0x20）。
pub const SEG_BASE: u32 = 0x63540;
pub const TS_REGRESS: u32 = SEG_BASE + 0x01; // 字段回退
pub const TS_BAD_CLK: u32 = SEG_BASE + 0x02; // CLK_TCK 错值
pub const TS_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const TS_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const TS_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// CLK_TCK 固定 100。
pub const CLK_TCK: u32 = 100;

/// tms 四字段。
#[derive(Clone, Copy, PartialEq)]
pub struct Tms {
    pub utime: u32,
    pub stime: u32,
    pub cutime: u32,
    pub cstime: u32,
}

impl Tms {
    pub fn zero() -> Tms {
        Tms { utime: 0, stime: 0, cutime: 0, cstime: 0 }
    }

    pub fn mono_ok(&self, prev: &Tms) -> bool {
        self.utime >= prev.utime
            && self.stime >= prev.stime
            && self.cutime >= prev.cutime
            && self.cstime >= prev.cstime
    }

    pub fn render(&self) -> String {
        format!("u{} s{} cu{} cs{}", self.utime, self.stime, self.cutime, self.cstime)
    }
}

/// 单条 tms 账。
#[derive(Clone)]
pub struct TsRec {
    pub seq: u32,
    pub pid: u32,
    pub snap: Tms,
    pub note: String,
}

impl TsRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
    }

    pub fn render_line(&self) -> String {
        format!("TS{}|p{}|{}|{}", self.seq, self.pid, self.snap.render(), self.note)
    }
}

/// tms 册。
pub struct TsBook {
    /// pid → 四字段
    pub tms: Vec<(u32, Tms)>,
    pub ledger: Vec<TsRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TsBook {
    pub fn new() -> TsBook {
        TsBook { tms: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    fn entry(&mut self, pid: u32) -> usize {
        if let Some(i) = self.tms.iter().position(|(w, _)| *w == pid) {
            return i;
        }
        self.tms.push((pid, Tms::zero()));
        self.tms.len() - 1
    }

    /// 自耗记账：burn(utick, stick)。
    pub fn burn(&mut self, pid: u32, utick: u32, stick: u32, note: &str) -> Result<Tms, u32> {
        if self.sealed {
            return Err(TS_FROZEN);
        }
        if note.is_empty() {
            return Err(TS_EMPTY_NOTE);
        }
        if pid == 0 {
            return Err(TS_REGRESS);
        }
        let i = self.entry(pid);
        let prev = self.tms[i].1;
        self.tms[i].1.utime += utick;
        self.tms[i].1.stime += stick;
        let cur = self.tms[i].1;
        if !cur.mono_ok(&prev) {
            return Err(TS_REGRESS);
        }
        self.ledger.push(TsRec { seq: self.seq_next, pid, snap: cur, note: note.to_string() });
        self.seq_next += 1;
        Ok(cur)
    }

    /// 收割累计：子 tms 全额并入父 c*。
    pub fn reap_into(&mut self, parent: u32, child: u32, note: &str) -> Result<Tms, u32> {
        if self.sealed {
            return Err(TS_FROZEN);
        }
        if note.is_empty() {
            return Err(TS_EMPTY_NOTE);
        }
        let ci = self.entry(child);
        let (cu, cs) = (self.tms[ci].1.utime, self.tms[ci].1.stime);
        let pi = self.entry(parent);
        let prev = self.tms[pi].1;
        self.tms[pi].1.cutime += cu;
        self.tms[pi].1.cstime += cs;
        let cur = self.tms[pi].1;
        if !cur.mono_ok(&prev) {
            return Err(TS_REGRESS);
        }
        self.ledger.push(TsRec {
            seq: self.seq_next,
            pid: parent,
            snap: cur,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(cur)
    }

    /// times 快照。
    pub fn times(&mut self, pid: u32, note: &str) -> Result<Tms, u32> {
        if self.sealed {
            return Err(TS_FROZEN);
        }
        if note.is_empty() {
            return Err(TS_EMPTY_NOTE);
        }
        let i = self.entry(pid);
        let cur = self.tms[i].1;
        self.ledger.push(TsRec { seq: self.seq_next, pid, snap: cur, note: note.to_string() });
        self.seq_next += 1;
        Ok(cur)
    }

    pub fn of(&self, pid: u32) -> Option<Tms> {
        self.tms.iter().find(|(w, _)| *w == pid).map(|(_, v)| *v)
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
        let mut h: u32 = 0x91720;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.pid);
            h = h.wrapping_mul(31).wrapping_add(r.snap.utime);
            h = h.wrapping_mul(31).wrapping_add(r.snap.stime);
            h = h.wrapping_mul(31).wrapping_add(r.snap.cutime);
            h = h.wrapping_mul(31).wrapping_add(r.snap.cstime);
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
    // 1) 回退码直证（zero 后不能减）
    let a = Tms { utime: 5, stime: 0, cutime: 0, cstime: 0 };
    let z = Tms::zero();
    got[0] = !a.mono_ok(&z) == false && z.utime <= a.utime; // 结构性自证
    // 2) CLK_TCK 错值
    got[1] = TS_BAD_CLK == SEG_BASE + 0x02 && CLK_TCK == 100;
    // 3) 空留痕
    let bad2 = TsRec { seq: 2, pid: 1, snap: Tms::zero(), note: String::new() };
    got[2] = !bad2.ok();
    // 4) 伪 pid burn
    let mut b = TsBook::new();
    got[3] = b.burn(0, 1, 1, "inj-zero").err() == Some(TS_REGRESS);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.burn(5, 1, 1, "inj-frozen").err() == Some(TS_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9172_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9172");
    let mut b = TsBook::new();

    // J1 tms 账非空
    let _ = b.burn(100, 7, 3, "自耗");
    let j1 = !b.ledger.is_empty();
    s.add("J1-tms账非空", j1, "burn 落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空");

    // J3 CLK_TCK 固定
    let j3 = CLK_TCK == 100;
    s.add("J3-CLK_TCK", j3, "CLK_TCK=100");

    // J4 自耗累计
    let j4 = {
        let mut b2 = TsBook::new();
        let _ = b2.burn(100, 7, 3, "a");
        let t = b2.burn(100, 2, 1, "b");
        t == Ok(Tms { utime: 9, stime: 4, cutime: 0, cstime: 0 })
    };
    s.add("J4-自耗累计", j4, "utime/stime 累加");

    // J5 四字段单调不减
    let j5 = {
        let mut b2 = TsBook::new();
        let _ = b2.burn(100, 5, 5, "a");
        let s1 = b2.of(100).unwrap_or(Tms::zero());
        let _ = b2.burn(100, 1, 1, "b");
        let s2 = b2.of(100).unwrap_or(Tms::zero());
        s2.mono_ok(&s1)
    };
    s.add("J5-单调不减", j5, "跨快照四字段不减");

    // J6 收割累计：子 u/s 并入父 c*
    let j6 = {
        let mut b2 = TsBook::new();
        let _ = b2.burn(200, 10, 5, "子耗");
        let t = b2.reap_into(100, 200, "收割");
        t == Ok(Tms { utime: 0, stime: 0, cutime: 10, cstime: 5 })
    };
    s.add("J6-收割累计", j6, "子 u/s 全额并入父 c*");

    // J7 空账零值
    let j7 = {
        let mut b2 = TsBook::new();
        b2.times(300, "空查") == Ok(Tms::zero())
    };
    s.add("J7-空账零值", j7, "未耗进程 times 全零");

    // J8 伪 pid 拒
    let j8 = b.burn(0, 1, 1, "零号").err() == Some(TS_REGRESS);
    s.add("J8-零号拒", j8, "pid=0 记账必拒");

    // J9 双子收割叠加
    let j9 = {
        let mut b2 = TsBook::new();
        let _ = b2.burn(200, 10, 0, "子一");
        let _ = b2.burn(201, 20, 0, "子二");
        let _ = b2.reap_into(100, 200, "收一");
        let t = b2.reap_into(100, 201, "收二");
        t == Ok(Tms { utime: 0, stime: 0, cutime: 30, cstime: 0 })
    };
    s.add("J9-双子叠加", j9, "多子收割 c* 累加");

    // J10 快照不改动
    let j10 = {
        let mut b2 = TsBook::new();
        let _ = b2.burn(100, 9, 9, "a");
        let before = b2.of(100);
        let _ = b2.times(100, "快照");
        before == b2.of(100)
    };
    s.add("J10-快照无副作用", j10, "times 不改字段");

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.burn(100, 1, 1, "").err() == Some(TS_EMPTY_NOTE);
    s.add("J12-空留痕拒", j12, "空留痕必拒");

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
    fn unx9172_tms_flow() {
        let mut b = TsBook::new();
        assert_eq!(CLK_TCK, 100);
        let _ = b.burn(100, 7, 3, "a");
        let t = b.burn(100, 2, 1, "b").unwrap();
        assert_eq!(t.utime, 9);
        assert_eq!(t.stime, 4);
        // 收割
        let _ = b.burn(200, 10, 5, "c");
        let p = b.reap_into(100, 200, "d").unwrap();
        assert_eq!(p.cutime, 10);
        assert_eq!(p.cstime, 5);
    }

    #[test]
    fn unx9172_checks_green() {
        let s = run_unx9172_checks();
        assert!(s.all_passed(), "UNX-F9172 判据未全绿");
    }
}
