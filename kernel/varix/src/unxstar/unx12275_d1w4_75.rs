//! UNX-F12275 文件族对照表 30 条批入账（0x64960 段）
//!
//! 卡面：30 条文件族对照表批入账——固定 30 槽（0..30）对照条目，每条
//! 场景号 + 期望码 + 实测码，实测与期望一致才允许入账；30 槽齐才可
//! seal；槽号越界/重号/不符皆拒。
//! 域：CoRun·UNX-D1-B14。
//!
//! 判据原文：
//! - J1 槽号闭集：0..30 之外必拒。
//! - J2 期望一致：实测≠期望入账必拒。
//! - J3 重号槽必拒。
//! - J4 未齐不可 seal：不足 30 条印账必拒。
//! - J5 齐账 seal 成功且指纹稳定。
//! - J6 印账冻结：seal 后补账必拒。
//! - J7 实测码可回读。
//! - J8 指纹幂等。
//! - J9 注入五况全抓。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x64960 独占 0x20）。
pub const SEG_BASE: u32 = 0x64960;
pub const TB_IDX_RANGE: u32 = SEG_BASE + 0x01; // 槽号越界
pub const TB_MISMATCH: u32 = SEG_BASE + 0x02; // 实测与期望不符
pub const TB_DUP: u32 = SEG_BASE + 0x03; // 重号槽
pub const TB_INCOMPLETE: u32 = SEG_BASE + 0x04; // 未齐印账
pub const TB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后补账

/// 固定槽位数。
pub const TB_SLOTS: usize = 30;

/// 30 条对照表期望口径（按槽号纯函数）。
pub fn expect_at(idx: usize) -> u32 {
    0x1000 + idx as u32 * 3
}

/// 一条对照条目。
#[derive(Clone)]
pub struct TblRec {
    pub idx: usize,
    pub got: u32,
}

/// 对照表账册（定容 30）。
pub struct TblBook {
    pub recs: Vec<TblRec>,
    pub sealed: bool,
}

impl TblBook {
    pub fn new() -> TblBook {
        TblBook { recs: Vec::new(), sealed: false }
    }

    fn pos(&self, idx: usize) -> Option<usize> {
        self.recs.iter().position(|r| r.idx == idx)
    }

    /// 入账：槽号闭集、期望一致、不得重号。
    pub fn put(&mut self, idx: usize, got: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TB_FROZEN);
        }
        if idx >= TB_SLOTS {
            return Err(TB_IDX_RANGE);
        }
        if got != expect_at(idx) {
            return Err(TB_MISMATCH);
        }
        if self.pos(idx).is_some() {
            return Err(TB_DUP);
        }
        self.recs.push(TblRec { idx, got });
        Ok(())
    }

    pub fn full(&self) -> bool {
        self.recs.len() == TB_SLOTS
    }

    pub fn seal(&mut self) -> Result<u32, u32> {
        if !self.full() {
            return Err(TB_INCOMPLETE);
        }
        self.sealed = true;
        Ok(self.fold())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.idx as u32);
            h = h.wrapping_mul(211).wrapping_add(r.got);
        }
        h
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("rows={}/{}", self.recs.len(), TB_SLOTS)
    }

    pub fn got_of(&self, idx: usize) -> Result<u32, u32> {
        match self.pos(idx) {
            Some(i) => Ok(self.recs[i].got),
            None => Err(TB_IDX_RANGE),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = TblBook::new();
    // 1) 槽号越界必拒。
    got[0] = b.put(30, expect_at(30)).err() == Some(TB_IDX_RANGE);
    // 2) 实测不符必拒。
    got[1] = b.put(0, 0).err() == Some(TB_MISMATCH);
    // 3) 重号槽必拒。
    let _ = b.put(0, expect_at(0));
    got[2] = b.put(0, expect_at(0)).err() == Some(TB_DUP);
    // 4) 未齐印账必拒。
    got[3] = b.seal().err() == Some(TB_INCOMPLETE);
    // 5) 印账后补账必抓。
    for idx in 0..TB_SLOTS {
        let _ = b.put(idx, expect_at(idx));
    }
    let _ = b.seal();
    got[4] = b.put(1, expect_at(1)).err() == Some(TB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12275_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12275");

    // J1 期望口径稳定。
    let j1 = expect_at(0) == 0x1000 && expect_at(29) == 0x1000 + 29 * 3;
    s.add("J1-期望口径", j1, "纯函数稳定");

    // J2 槽号闭集。
    let j2 = {
        let mut b = TblBook::new();
        b.put(30, 0).err() == Some(TB_IDX_RANGE)
            && b.put(usize::MAX, 0).err() == Some(TB_IDX_RANGE)
    };
    s.add("J2-槽号闭集", j2, "0..30 之外必拒");

    // J3 期望一致。
    let j3 = {
        let mut b = TblBook::new();
        b.put(5, expect_at(5) + 1).err() == Some(TB_MISMATCH)
            && b.put(5, expect_at(5)).is_ok()
    };
    s.add("J3-期望一致", j3, "不符拒、相符收");

    // J4 重号槽拒。
    let j4 = {
        let mut b = TblBook::new();
        let _ = b.put(7, expect_at(7));
        b.put(7, expect_at(7)).err() == Some(TB_DUP)
    };
    s.add("J4-重号槽拒", j4, "槽唯一");

    // J5 未齐印账拒。
    let j5 = {
        let mut b = TblBook::new();
        for idx in 0..TB_SLOTS - 1 {
            let _ = b.put(idx, expect_at(idx));
        }
        b.seal().err() == Some(TB_INCOMPLETE)
    };
    s.add("J5-未齐拒", j5, "差一条都不行");

    // J6 齐账 seal 成功。
    let j6 = {
        let mut b = TblBook::new();
        for idx in 0..TB_SLOTS {
            let _ = b.put(idx, expect_at(idx));
        }
        b.seal().is_ok() && b.summary().contains("rows=30/30")
    };
    s.add("J6-齐账seal", j6, "30 槽全满");

    // J7 印账冻结。
    let j7 = {
        let mut b = TblBook::new();
        for idx in 0..TB_SLOTS {
            let _ = b.put(idx, expect_at(idx));
        }
        let _ = b.seal();
        b.put(0, expect_at(0)).err() == Some(TB_FROZEN)
    };
    s.add("J7-印账冻结", j7, "seal 后必拒");

    // J8 实测码回读。
    let j8 = {
        let mut b = TblBook::new();
        let _ = b.put(9, expect_at(9));
        b.got_of(9).ok() == Some(expect_at(9))
            && b.got_of(29).err() == Some(TB_IDX_RANGE)
    };
    s.add("J8-实测回读", j8, "在册命中、缺行拒");

    // J9 指纹幂等。
    let j9 = {
        let mut b = TblBook::new();
        let _ = b.put(3, expect_at(3));
        b.fold() == b.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 顺序无关：乱序填满与顺序填满指纹一致。
    let j10 = {
        let mut b1 = TblBook::new();
        let mut b2 = TblBook::new();
        for idx in 0..TB_SLOTS {
            let _ = b1.put(idx, expect_at(idx));
        }
        for idx in (0..TB_SLOTS).rev() {
            let _ = b2.put(idx, expect_at(idx));
        }
        b1.fold() == b2.fold()
    };
    s.add("J10-顺序无关", j10, "指纹只看内容");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = TblBook::new();
    let _ = b.put(0, expect_at(0));
    let _ = b.put(1, expect_at(1));
    s.add("摘要回读", b.summary().contains("rows=2/30"), "双行在册");
    s.add("双槽核对", b.got_of(0).ok() == Some(expect_at(0)), "实测回读一致");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12275_table_flow() {
        let mut b = TblBook::new();
        assert!(b.put(0, expect_at(0)).is_ok());
        assert_eq!(b.put(0, expect_at(0)), Err(TB_DUP));
        assert_eq!(b.put(TB_SLOTS, 0), Err(TB_IDX_RANGE));
        assert_eq!(b.put(1, 0), Err(TB_MISMATCH));
    }

    #[test]
    fn unx12275_checks_green() {
        let s = run_unx12275_checks();
        assert!(s.all_passed(), "UNX-F12275 判据未全绿");
    }

    #[test]
    fn unx12275_inject_all_caught() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "注入五况未全抓: {:?}", got);
    }
}
