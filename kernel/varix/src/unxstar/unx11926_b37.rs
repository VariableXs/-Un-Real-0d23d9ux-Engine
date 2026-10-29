//! UNX-F11926 I 型七批清账判据（0x42C20 段）
//!
//! 锚点：UNX-F11926-J1 I 型迭代清账面判据绿（七轮逐清/轮次单调/账
//! 平），与基准机一致；J2 I 型语义（轮次乱序拒 + 轮次重复拒 + 印后
//! 拒三性质）；J3 清账指纹（账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C5-B37（I 型清账面）。
//!
//! 结构：七轮迭代清账器 + 轮次闸核 + 逐批清核 + 账本 + 折叠指纹 +
//! 注入核（乱轮/重轮/野批/印后/红残留必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x42C20 独占 0x20）。
pub const SEG_BASE: u32 = 0x42C20;
pub const IC_ROUND_SKIP: u32 = SEG_BASE + 0x01; // 轮次乱序
pub const IC_ROUND_DUP: u32 = SEG_BASE + 0x02; // 轮次重复
pub const IC_BAD_BATCH: u32 = SEG_BASE + 0x03; // 野批号
pub const IC_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const IC_RED_LEFT: u32 = SEG_BASE + 0x05; // 红残留

/// I 型七轮迭代清账器：轮次必须 1..=7 严格递增。
pub struct IcLedger {
    reds: [u32; 7],
    closed: [bool; 7],
    rounds_done: u8,
    sealed: bool,
}

impl IcLedger {
    pub const ROUNDS: u8 = 7;

    pub fn new() -> IcLedger {
        IcLedger {
            reds: [0; 7],
            closed: [false; 7],
            rounds_done: 0,
            sealed: false,
        }
    }

    pub fn record(&mut self, batch: u8, reds: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IC_FROZEN);
        }
        if batch as usize >= 7 {
            return Err(IC_BAD_BATCH);
        }
        self.reds[batch as usize] = self.reds[batch as usize].saturating_add(reds);
        Ok(())
    }

    /// 轮次清账：round 必须 = rounds_done+1；每轮清一批。
    pub fn step(&mut self, round: u8, batch: u8) -> Result<u32, u32> {
        if self.sealed {
            return Err(IC_FROZEN);
        }
        if round == 0 || round > Self::ROUNDS {
            return Err(IC_ROUND_SKIP);
        }
        if round <= self.rounds_done {
            return Err(IC_ROUND_DUP);
        }
        if round != self.rounds_done + 1 {
            return Err(IC_ROUND_SKIP);
        }
        if batch as usize >= 7 {
            return Err(IC_BAD_BATCH);
        }
        if self.closed[batch as usize] {
            return Err(IC_ROUND_DUP);
        }
        let r = self.reds[batch as usize];
        if r > 0 {
            return Err(IC_RED_LEFT);
        }
        self.closed[batch as usize] = true;
        self.rounds_done = round;
        Ok(r)
    }

    /// 轮次强清（清红并闭）。
    pub fn step_force(&mut self, round: u8, batch: u8) -> Result<u32, u32> {
        if self.sealed {
            return Err(IC_FROZEN);
        }
        if round == 0 || round > Self::ROUNDS {
            return Err(IC_ROUND_SKIP);
        }
        if round <= self.rounds_done {
            return Err(IC_ROUND_DUP);
        }
        if round != self.rounds_done + 1 {
            return Err(IC_ROUND_SKIP);
        }
        if batch as usize >= 7 {
            return Err(IC_BAD_BATCH);
        }
        if self.closed[batch as usize] {
            return Err(IC_ROUND_DUP);
        }
        let r = self.reds[batch as usize];
        self.reds[batch as usize] = 0;
        self.closed[batch as usize] = true;
        self.rounds_done = round;
        Ok(r)
    }

    pub fn balanced(&self) -> bool {
        self.rounds_done == Self::ROUNDS
            && self.closed.iter().all(|c| *c)
            && self.reds.iter().all(|r| *r == 0)
    }

    pub fn rounds(&self) -> u8 {
        self.rounds_done
    }

    pub fn red_total(&self) -> u32 {
        self.reds.iter().sum()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x42C20;
        h = h.wrapping_mul(31).wrapping_add(self.rounds_done as u32);
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

/// 标准夹具：七轮顺序强清（批号=轮号-1），账平。
pub fn mk_ledger() -> IcLedger {
    let mut l = IcLedger::new();
    for b in 0..7u8 {
        let _ = l.record(b, 1);
        let _ = l.step_force(b + 1, b);
    }
    l
}

/// 注入核：五个必然被拒的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 轮次乱序（跳轮）
    let mut l = IcLedger::new();
    got[0] = l.step_force(2, 0).err() == Some(IC_ROUND_SKIP);
    // 2) 轮次重复
    let mut l2 = IcLedger::new();
    let _ = l2.step_force(1, 0);
    got[1] = l2.step_force(1, 1).err() == Some(IC_ROUND_DUP);
    // 3) 野批号
    got[2] = IcLedger::new().record(7, 1).err() == Some(IC_BAD_BATCH);
    // 4) 红残留轮清
    let mut l3 = IcLedger::new();
    let _ = l3.record(0, 6);
    got[3] = l3.step(1, 0).err() == Some(IC_RED_LEFT);
    // 5) 印后操作
    let mut l4 = mk_ledger();
    l4.seal();
    got[4] = l4.record(0, 1).err() == Some(IC_FROZEN)
        && l4.step_force(1, 0).err() == Some(IC_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx11926_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11926");
    let mut l = mk_ledger();

    // J1 七轮全走
    let j1 = l.rounds() == 7;
    s.add("J1-七轮全走", j1, "轮 7/7");

    // J2 账平
    let j2 = l.balanced();
    s.add("J2-账平", j2, "全闭红零");

    // J3 红零
    let j3 = l.red_total() == 0;
    s.add("J3-红零", j3, "红目 0");

    // J4 乱轮拒（跳轮）
    let j4 = {
        let mut l2 = IcLedger::new();
        let _ = l2.step_force(1, 0);
        l2.step_force(3, 1).err() == Some(IC_ROUND_SKIP)
    };
    s.add("J4-乱轮拒", j4, "跳到轮 3 必拒");

    // J5 重轮拒
    let j5 = {
        let mut l2 = IcLedger::new();
        let _ = l2.step_force(1, 0);
        l2.step_force(1, 1).err() == Some(IC_ROUND_DUP)
    };
    s.add("J5-重轮拒", j5, "轮 1 重走必拒");

    // J6 零轮拒
    let j6 = IcLedger::new().step(0, 0).err() == Some(IC_ROUND_SKIP);
    s.add("J6-零轮拒", j6, "round=0 必拒");

    // J7 野批号拒
    let j7 = l.record(7, 1).err() == Some(IC_BAD_BATCH)
        && l.step(1, 7).err() == Some(IC_BAD_BATCH);
    s.add("J7-野批拒", j7, "批号≥7 必拒");

    // J8 红残留轮清拒
    let j8 = {
        let mut l2 = IcLedger::new();
        let _ = l2.record(2, 4);
        l2.step(1, 2).err() == Some(IC_RED_LEFT)
    };
    s.add("J8-红残留拒", j8, "红未清闭批必拒");

    // J9 强清带值
    let j9 = {
        let mut l2 = IcLedger::new();
        let _ = l2.record(5, 9);
        l2.step_force(1, 5) == Ok(9)
    };
    s.add("J9-强清带值", j9, "强清返旧红数");

    // J10 轮批解耦：任意批任意轮（批不重复即可）
    let j10 = {
        let mut l2 = IcLedger::new();
        let a = l2.step_force(1, 6).is_ok();
        let b = l2.step_force(2, 6).is_err(); // 批 6 已闭
        a && b
    };
    s.add("J10-批不重闭", j10, "同批二次闭拒");

    // J11 印后操作拒
    let j11 = {
        let mut l2 = mk_ledger();
        l2.seal();
        l2.step(1, 0).err() == Some(IC_FROZEN)
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
    fn unx11926_round_flow() {
        let mut l = IcLedger::new();
        assert_eq!(l.step(0, 0), Err(IC_ROUND_SKIP));
        assert_eq!(l.step(2, 0), Err(IC_ROUND_SKIP));
        assert_eq!(l.step(1, 7), Err(IC_BAD_BATCH));
        assert_eq!(l.record(0, 5), Ok(()));
        assert_eq!(l.step(1, 0), Err(IC_RED_LEFT));
        assert_eq!(l.step_force(1, 0), Ok(5));
        assert_eq!(l.step_force(1, 1), Err(IC_ROUND_DUP));
        assert_eq!(l.step_force(2, 0), Err(IC_ROUND_DUP));
        for r in 2..=7u8 {
            let _ = l.step_force(r, r - 1);
        }
        assert!(l.balanced());
        l.seal();
        assert_eq!(l.step_force(1, 0), Err(IC_FROZEN));
    }

    #[test]
    fn unx11926_checks_green() {
        let s = run_unx11926_checks();
        assert!(s.all_passed(), "UNX-F11926 判据未全绿");
    }
}
