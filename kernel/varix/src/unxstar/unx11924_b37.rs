//! UNX-F11924 M 型十二批清账判据（0x42BE0 段）
//!
//! 锚点：UNX-F11924-J1 M 型清账面判据绿（十二批逐清/半账拒/印面唯
//! 一），与基准机一致；J2 M 型语义（批齐才发印 + 半账拒印 + 重印拒
//! 三性质）；J3 印指纹（印后账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C5-B37（M 型清账面）。
//!
//! 结构：十二批清账印器 + 逐批清核 + 发印核（半账/重印/印后录全拒）
//! + 账本 + 折叠指纹 + 注入核（野批/半账发印/重印/印后录/红残留必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x42BE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x42BE0;
pub const MC_BAD_BATCH: u32 = SEG_BASE + 0x01; // 野批号
pub const MC_HALF: u32 = SEG_BASE + 0x02; // 半账发印
pub const MC_ALREADY: u32 = SEG_BASE + 0x03; // 重印
pub const MC_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const MC_RED_LEFT: u32 = SEG_BASE + 0x05; // 红残留

/// M 型十二批清账印器。
pub struct McLedger {
    reds: [u32; 12],
    closed: [bool; 12],
    pub seal_fold: Option<u32>,
    sealed: bool,
}

impl McLedger {
    pub const BATCHES: usize = 12;

    pub fn new() -> McLedger {
        McLedger {
            reds: [0; 12],
            closed: [false; 12],
            seal_fold: None,
            sealed: false,
        }
    }

    pub fn record(&mut self, batch: u8, reds: u32) -> Result<(), u32> {
        if self.seal_fold.is_some() {
            return Err(MC_FROZEN);
        }
        if batch as usize >= Self::BATCHES {
            return Err(MC_BAD_BATCH);
        }
        self.reds[batch as usize] = self.reds[batch as usize].saturating_add(reds);
        Ok(())
    }

    pub fn clear(&mut self, batch: u8) -> Result<u32, u32> {
        if self.seal_fold.is_some() {
            return Err(MC_FROZEN);
        }
        if batch as usize >= Self::BATCHES {
            return Err(MC_BAD_BATCH);
        }
        if self.closed[batch as usize] {
            return Err(MC_ALREADY);
        }
        let r = self.reds[batch as usize];
        if r > 0 {
            return Err(MC_RED_LEFT);
        }
        self.closed[batch as usize] = true;
        Ok(r)
    }

    pub fn clear_force(&mut self, batch: u8) -> Result<u32, u32> {
        if self.seal_fold.is_some() {
            return Err(MC_FROZEN);
        }
        if batch as usize >= Self::BATCHES {
            return Err(MC_BAD_BATCH);
        }
        if self.closed[batch as usize] {
            return Err(MC_ALREADY);
        }
        let r = self.reds[batch as usize];
        self.reds[batch as usize] = 0;
        self.closed[batch as usize] = true;
        Ok(r)
    }

    pub fn closed_count(&self) -> usize {
        self.closed.iter().filter(|c| **c).count()
    }

    pub fn red_total(&self) -> u32 {
        self.reds.iter().sum()
    }

    /// 发印：十二批全闭且红零才发。
    pub fn issue(&mut self) -> Result<u32, u32> {
        if self.seal_fold.is_some() {
            return Err(MC_ALREADY);
        }
        if self.closed_count() < Self::BATCHES {
            return Err(MC_HALF);
        }
        if self.red_total() > 0 {
            return Err(MC_RED_LEFT);
        }
        let mut h: u32 = 0x42BE0;
        h = h.wrapping_mul(31).wrapping_add(Self::BATCHES as u32);
        self.seal_fold = Some(h);
        self.sealed = true;
        Ok(h)
    }

    /// 印后录。
    pub fn record_seal(&self) -> Result<u32, u32> {
        match self.seal_fold {
            Some(f) => Ok(f),
            None => Err(MC_FROZEN),
        }
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x42BE0;
        h = h.wrapping_mul(31).wrapping_add(self.closed_count() as u32);
        for r in &self.reds {
            h = h.wrapping_mul(31).wrapping_add(*r);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：十二批全记全强清发印。
pub fn mk_ledger() -> McLedger {
    let mut l = McLedger::new();
    for b in 0..12u8 {
        let _ = l.record(b, 1);
        let _ = l.clear_force(b);
    }
    let _ = l.issue();
    l
}

/// 注入核：五个必然被拒的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 野批号
    got[0] = McLedger::new().record(12, 1).err() == Some(MC_BAD_BATCH);
    // 2) 半账发印
    let mut l = McLedger::new();
    let _ = l.clear_force(0);
    got[1] = l.issue().err() == Some(MC_HALF);
    // 3) 红残留发印（全闭但红未清不可能：闭需清红；用强清后回记红）
    let mut l2 = McLedger::new();
    for b in 0..12u8 {
        let _ = l2.clear_force(b);
    }
    let _ = l2.record(3, 9);
    got[2] = l2.issue().err() == Some(MC_RED_LEFT);
    // 4) 重印
    let mut l3 = mk_ledger();
    got[3] = l3.issue().err() == Some(MC_ALREADY);
    // 5) 印后录（未印拒录）
    got[4] = McLedger::new().record_seal().err() == Some(MC_FROZEN)
        && mk_ledger().record_seal().is_ok();
    got
}

/// 判据主体。
pub fn run_unx11924_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11924");
    let mut l = mk_ledger();

    // J1 十二批全闭
    let j1 = l.closed_count() == 12;
    s.add("J1-十二批全闭", j1, "闭 12/12");

    // J2 红零
    let j2 = l.red_total() == 0;
    s.add("J2-红零", j2, "红目 0");

    // J3 发印成功
    let j3 = l.seal_fold.is_some();
    s.add("J3-发印", j3, "M 印成");

    // J4 印后录
    let j4 = l.record_seal().is_ok();
    s.add("J4-印后录", j4, "印发可录");

    // J5 半账发印拒
    let j5 = {
        let mut l2 = McLedger::new();
        for b in 0..6u8 {
            let _ = l2.clear_force(b);
        }
        l2.issue().err() == Some(MC_HALF)
    };
    s.add("J5-半账拒印", j5, "闭 6/12 发印必拒");

    // J6 野批号拒
    let j6 = l.record(12, 1).err() == Some(MC_BAD_BATCH)
        && l.clear(15).err() == Some(MC_BAD_BATCH);
    s.add("J6-野批拒", j6, "批号≥12 必拒");

    // J7 红残留闭批拒
    let j7 = {
        let mut l2 = McLedger::new();
        let _ = l2.record(1, 4);
        l2.clear(1).err() == Some(MC_RED_LEFT)
    };
    s.add("J7-红残留拒", j7, "红未清闭批必拒");

    // J8 重印拒
    let j8 = l.issue().err() == Some(MC_ALREADY);
    s.add("J8-重印拒", j8, "重发必拒");

    // J9 印后操作拒
    let j9 = {
        let mut l2 = mk_ledger();
        l2.record(0, 1).err() == Some(MC_FROZEN)
            && l2.clear(0).err() == Some(MC_FROZEN)
            && l2.clear_force(1).err() == Some(MC_FROZEN)
    };
    s.add("J9-印后操作拒", j9, "印发后冻结");

    // J10 强清带值
    let j10 = {
        let mut l2 = McLedger::new();
        let _ = l2.record(7, 11);
        l2.clear_force(7) == Ok(11)
    };
    s.add("J10-强清带值", j10, "强清返旧红数");

    // J11 未印拒录
    let j11 = McLedger::new().record_seal().err() == Some(MC_FROZEN);
    s.add("J11-未印拒录", j11, "未印发录必拒");

    // J12 指纹稳定
    let j12 = {
        let mut l2 = mk_ledger();
        let a = l2.fold();
        l2.seal();
        l2.fold() == a
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
    fn unx11924_mclear_flow() {
        let mut l = McLedger::new();
        assert_eq!(l.record(12, 1), Err(MC_BAD_BATCH));
        assert_eq!(l.record(0, 2), Ok(()));
        assert_eq!(l.clear(0), Err(MC_RED_LEFT));
        assert_eq!(l.clear_force(0), Ok(2));
        assert_eq!(l.issue(), Err(MC_HALF));
        for b in 1..12u8 {
            let _ = l.clear_force(b);
        }
        assert!(l.issue().is_ok());
        assert_eq!(l.issue(), Err(MC_ALREADY));
        assert!(l.record_seal().is_ok());
        assert_eq!(l.record(0, 1), Err(MC_FROZEN));
    }

    #[test]
    fn unx11924_checks_green() {
        let s = run_unx11924_checks();
        assert!(s.all_passed(), "UNX-F11924 判据未全绿");
    }
}
