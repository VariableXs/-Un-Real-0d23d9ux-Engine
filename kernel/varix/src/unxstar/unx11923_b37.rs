//! UNX-F11923 F 型八批清账判据（0x42BC0 段）
//!
//! 锚点：UNX-F11923-J1 清账面判据绿（八批逐清/红零/账平），与基准
//! 机一致；J2 清账语义（批齐才账平 + 野批拒 + 重清拒三性质）；J3 清
//! 账指纹（账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C5-B37（F 型清账面）。
//!
//! 结构：八批清账器 + 逐批清核 + 账平核 + 账本 + 折叠指纹 + 注入核
//! （野批号/超清/印后/红残留/批缺必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x42BC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x42BC0;
pub const FC_BAD_BATCH: u32 = SEG_BASE + 0x01; // 野批号
pub const FC_RECLEARED: u32 = SEG_BASE + 0x02; // 超清
pub const FC_FROZEN: u32 = SEG_BASE + 0x03; // 印后操作
pub const FC_RED_LEFT: u32 = SEG_BASE + 0x04; // 红残留
pub const FC_BAD_RED: u32 = SEG_BASE + 0x05; // 红目格式坏

/// F 型八批清账器。
pub struct FcLedger {
    reds: [u32; 8],
    closed: [bool; 8],
    pub clears: u64,
    sealed: bool,
}

impl FcLedger {
    pub const BATCHES: usize = 8;

    pub fn new() -> FcLedger {
        FcLedger {
            reds: [0; 8],
            closed: [false; 8],
            clears: 0,
            sealed: false,
        }
    }

    /// 记红目。
    pub fn record(&mut self, batch: u8, reds: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(FC_FROZEN);
        }
        if batch as usize >= Self::BATCHES {
            return Err(FC_BAD_BATCH);
        }
        self.reds[batch as usize] = self.reds[batch as usize].saturating_add(reds);
        Ok(())
    }

    /// 清一批：红零才准闭，闭后重清拒。
    pub fn clear(&mut self, batch: u8) -> Result<u32, u32> {
        if self.sealed {
            return Err(FC_FROZEN);
        }
        if batch as usize >= Self::BATCHES {
            return Err(FC_BAD_BATCH);
        }
        if self.closed[batch as usize] {
            return Err(FC_RECLEARED);
        }
        let r = self.reds[batch as usize];
        if r > 0 {
            return Err(FC_RED_LEFT);
        }
        self.closed[batch as usize] = true;
        self.clears += 1;
        Ok(r)
    }

    /// 先清红再闭批：清空指定批红目并闭。
    pub fn clear_force(&mut self, batch: u8) -> Result<u32, u32> {
        if self.sealed {
            return Err(FC_FROZEN);
        }
        if batch as usize >= Self::BATCHES {
            return Err(FC_BAD_BATCH);
        }
        if self.closed[batch as usize] {
            return Err(FC_RECLEARED);
        }
        let r = self.reds[batch as usize];
        self.reds[batch as usize] = 0;
        self.closed[batch as usize] = true;
        self.clears += 1;
        Ok(r)
    }

    /// 账平：八批全闭且红零。
    pub fn balanced(&self) -> bool {
        self.closed.iter().all(|c| *c) && self.reds.iter().all(|r| *r == 0)
    }

    pub fn red_total(&self) -> u32 {
        self.reds.iter().sum()
    }

    pub fn closed_count(&self) -> usize {
        self.closed.iter().filter(|c| **c).count()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x42BC0;
        h = h.wrapping_mul(31).wrapping_add(self.clears as u32);
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

/// 标准夹具：八批各记 1 红再强制清，全账平。
pub fn mk_ledger() -> FcLedger {
    let mut l = FcLedger::new();
    for b in 0..8u8 {
        let _ = l.record(b, 1);
        let _ = l.clear_force(b);
    }
    l
}

/// 注入核：五个必然被拒的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 野批号
    got[0] = FcLedger::new().record(8, 1).err() == Some(FC_BAD_BATCH);
    // 2) 红残留闭批
    let mut l = FcLedger::new();
    let _ = l.record(0, 3);
    got[1] = l.clear(0).err() == Some(FC_RED_LEFT);
    // 3) 超清
    let mut l2 = mk_ledger();
    got[2] = l2.clear_force(0).err() == Some(FC_RECLEARED);
    // 4) 账平前置：红不清则不平
    let mut l3 = FcLedger::new();
    let _ = l3.record(1, 2);
    let _ = l3.clear_force(0);
    got[3] = !l3.balanced() && l3.red_total() == 2;
    // 5) 印后操作
    let mut l4 = mk_ledger();
    l4.seal();
    got[4] = l4.record(0, 1).err() == Some(FC_FROZEN) && l4.clear(0).err() == Some(FC_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx11923_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11923");
    let mut l = mk_ledger();

    // J1 八批全闭
    let j1 = l.closed_count() == 8;
    s.add("J1-八批全闭", j1, "闭 8/8");

    // J2 红零
    let j2 = l.red_total() == 0;
    s.add("J2-红零", j2, "红目 0");

    // J3 账平
    let j3 = l.balanced();
    s.add("J3-账平", j3, "全闭且红零");

    // J4 清账计数
    let j4 = l.clears == 8;
    s.add("J4-清账计数", j4, "清 8 次");

    // J5 野批号拒
    let j5 = l.record(8, 1).err() == Some(FC_BAD_BATCH)
        && l.clear(9).err() == Some(FC_BAD_BATCH);
    s.add("J5-野批拒", j5, "批号≥8 必拒");

    // J6 红残留闭批拒
    let j6 = {
        let mut l2 = FcLedger::new();
        let _ = l2.record(3, 5);
        l2.clear(3).err() == Some(FC_RED_LEFT)
    };
    s.add("J6-红残留拒", j6, "红未清闭批必拒");

    // J7 强清路径：清红带出旧值
    let j7 = {
        let mut l2 = FcLedger::new();
        let _ = l2.record(2, 7);
        l2.clear_force(2) == Ok(7)
    };
    s.add("J7-强清带值", j7, "强清返旧红数");

    // J8 重清拒
    let j8 = l.clear(0).err() == Some(FC_RECLEARED);
    s.add("J8-重清拒", j8, "已闭再清必拒");

    // J9 部分闭不算平
    let j9 = {
        let mut l2 = FcLedger::new();
        let _ = l2.clear_force(0);
        !l2.balanced()
    };
    s.add("J9-部分闭不平", j9, "闭 1/8 不平");

    // J10 记账累加
    let j10 = {
        let mut l2 = FcLedger::new();
        let _ = l2.record(5, 2);
        let _ = l2.record(5, 3);
        l2.red_total() == 5
    };
    s.add("J10-记账累加", j10, "同批红累加");

    // J11 印后操作拒
    let j11 = {
        let mut l2 = mk_ledger();
        l2.seal();
        l2.record(0, 1).err() == Some(FC_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

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
    fn unx11923_clear_flow() {
        let mut l = FcLedger::new();
        assert_eq!(l.record(8, 1), Err(FC_BAD_BATCH));
        assert_eq!(l.record(0, 4), Ok(()));
        assert_eq!(l.clear(0), Err(FC_RED_LEFT));
        assert_eq!(l.clear_force(0), Ok(4));
        assert_eq!(l.clear(0), Err(FC_RECLEARED));
        assert!(!l.balanced());
        assert_eq!(l.red_total(), 0);
        l.seal();
        assert_eq!(l.record(1, 1), Err(FC_FROZEN));
        assert_eq!(l.clear(1), Err(FC_FROZEN));
    }

    #[test]
    fn unx11923_checks_green() {
        let s = run_unx11923_checks();
        assert!(s.all_passed(), "UNX-F11923 判据未全绿");
    }
}
