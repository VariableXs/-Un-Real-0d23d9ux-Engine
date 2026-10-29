//! UNX-F24195 vblank/HPD 内核事件面与事件队列·子功能15（连接账本）（0x41A40 段）
//!
//! 锚点：UNX-F24195-J1 生命周期账（连接/使能/断开逐条落账）；J2 账
//! 本语义（账序守恒 + 野连接器拒 + 重复条目拒三性质）；J3 账本指纹
//! （账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（连接账本面）。
//!
//! 结构：连接账本 + 记账核 + 对账核 + 账本 + 折叠指纹 + 注入核（野
//! 连接器/重复账/印后/空账对账/超容必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41A40 独占 0x20）。
pub const SEG_BASE: u32 = 0x41A40;
pub const LB_BAD_CONN: u32 = SEG_BASE + 0x01; // 野连接器
pub const LB_DUP: u32 = SEG_BASE + 0x02; // 重复账目
pub const LB_FROZEN: u32 = SEG_BASE + 0x03; // 印后操作
pub const LB_OVERFLOW: u32 = SEG_BASE + 0x04; // 账目超容
pub const LB_BAD_SEQ: u32 = SEG_BASE + 0x05; // 账序错乱

/// 账目。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LbEntry {
    pub conn: u64,
    pub act: u8, // 0=连接 1=使能 2=断开
    pub seq: u64,
}

/// 连接账本。
pub struct LbBook {
    pub cap: usize,
    entries: Vec<LbEntry>,
    next_seq: u64,
    sealed: bool,
}

impl LbBook {
    pub fn new(cap: usize) -> LbBook {
        LbBook {
            cap,
            entries: Vec::new(),
            next_seq: 1,
            sealed: false,
        }
    }

    /// 记账：seq 自动递增；野连接器（0）拒。
    pub fn record(&mut self, conn: u64, act: u8) -> Result<u64, u32> {
        if self.sealed {
            return Err(LB_FROZEN);
        }
        if conn == 0 {
            return Err(LB_BAD_CONN);
        }
        if act > 2 {
            return Err(LB_BAD_SEQ);
        }
        if self.entries.len() >= self.cap {
            return Err(LB_OVERFLOW);
        }
        if let Some(last) = self.entries.last() {
            if last.conn == conn && last.act == act {
                return Err(LB_DUP);
            }
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.entries.push(LbEntry { conn, act, seq });
        Ok(seq)
    }

    /// 对账：seq 严格递增 1..=n。
    pub fn audit(&self) -> bool {
        for (i, e) in self.entries.iter().enumerate() {
            if e.seq != (i as u64) + 1 {
                return false;
            }
        }
        true
    }

    /// 某连接器的账目数。
    pub fn entries_of(&self, conn: u64) -> usize {
        self.entries.iter().filter(|e| e.conn == conn).count()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn snapshot(&self) -> Vec<LbEntry> {
        self.entries.iter().copied().collect()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41A40;
        h = h.wrapping_mul(31).wrapping_add(self.next_seq as u32);
        for e in &self.entries {
            h = h.wrapping_mul(31).wrapping_add(e.conn as u32);
            h = h.wrapping_mul(31).wrapping_add(e.act as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：连接器 7 三段生命周期 + 连接器 9 一次连接。
pub fn mk_book() -> LbBook {
    let mut b = LbBook::new(16);
    let _ = b.record(7, 0); // 连接
    let _ = b.record(7, 1); // 使能
    let _ = b.record(7, 2); // 断开
    let _ = b.record(9, 0);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 野连接器
    got[0] = LbBook::new(8).record(0, 0).err() == Some(LB_BAD_CONN);
    // 2) 野动作
    got[1] = LbBook::new(8).record(1, 9).err() == Some(LB_BAD_SEQ);
    // 3) 重复账目（同连接器同动作连续）
    let mut b = LbBook::new(8);
    let _ = b.record(5, 0);
    got[2] = b.record(5, 0).err() == Some(LB_DUP);
    // 4) 超容
    let mut s = LbBook::new(1);
    let _ = s.record(1, 0);
    got[3] = s.record(2, 0).err() == Some(LB_OVERFLOW);
    // 5) 印后操作
    let mut b2 = mk_book();
    b2.seal();
    got[4] = b2.record(7, 0).err() == Some(LB_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24195_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24195");
    let mut b = mk_book();

    // J1 账目数：4
    let j1 = b.len() == 4;
    s.add("J1-账目数", j1, "记 4 笔");

    // J2 对账通过：seq 1..=4
    let j2 = b.audit();
    s.add("J2-对账", j2, "seq 严格递增");

    // J3 连接器 7 账目 3
    let j3 = b.entries_of(7) == 3;
    s.add("J3-按连器查", j3, "7 号三笔");

    // J4 账序快照：首笔 (7,0,1)
    let j4 = {
        let snap = b.snapshot();
        snap[0] == LbEntry { conn: 7, act: 0, seq: 1 }
    };
    s.add("J4-首笔", j4, "首笔连接");

    // J5 野连接器拒
    let j5 = b.record(0, 0).err() == Some(LB_BAD_CONN);
    s.add("J5-野连接器拒", j5, "conn=0 必拒");

    // J6 野动作拒
    let j6 = b.record(7, 9).err() == Some(LB_BAD_SEQ);
    s.add("J6-野动作拒", j6, "act>2 必拒");

    // J7 重复账目拒
    let j7 = {
        let mut b2 = LbBook::new(8);
        let _ = b2.record(3, 1);
        b2.record(3, 1).err() == Some(LB_DUP)
    };
    s.add("J7-重复账拒", j7, "连续同账必拒");

    // J8 异连接器同动作放行
    let j8 = {
        let mut b2 = LbBook::new(8);
        let _ = b2.record(3, 1);
        b2.record(4, 1).is_ok()
    };
    s.add("J8-异器放行", j8, "异连接器不视重");

    // J9 超容拒
    let j9 = {
        let mut b2 = LbBook::new(2);
        let _ = b2.record(1, 0);
        let _ = b2.record(2, 0);
        b2.record(3, 0).err() == Some(LB_OVERFLOW)
    };
    s.add("J9-超容拒", j9, "满账必拒");

    // J10 seq 守恒：续记接 5
    let j10 = b.record(7, 0) == Ok(5);
    s.add("J10-seq守恒", j10, "续记 seq=5");

    // J11 印后操作拒
    let j11 = {
        let mut b2 = mk_book();
        b2.seal();
        b2.record(7, 0).err() == Some(LB_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut b2 = mk_book();
        let a = b2.fold();
        b2.seal();
        b2.fold() == a
    };
    s.add("J12-指纹稳定", j12, "印不改指纹");

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
    fn unx24195_ledger_flow() {
        let mut b = LbBook::new(4);
        assert_eq!(b.record(7, 0), Ok(1));
        assert_eq!(b.record(7, 0), Err(LB_DUP));
        assert_eq!(b.record(0, 0), Err(LB_BAD_CONN));
        assert_eq!(b.record(7, 9), Err(LB_BAD_SEQ));
        assert_eq!(b.record(9, 0), Ok(2));
        assert!(b.audit());
        assert_eq!(b.entries_of(9), 1);
        b.seal();
        assert_eq!(b.record(9, 0), Err(LB_FROZEN));
    }

    #[test]
    fn unx24195_checks_green() {
        let s = run_unx24195_checks();
        assert!(s.all_passed(), "UNX-F24195 判据未全绿");
    }
}
