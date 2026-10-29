//! UNX-F12031 STATUS_MEMORY_* 内存错误族（A4 联签）（0x19600 段）
//!
//! 判据原文：提交账面——提交（commit）不得越承诺上限（COMMITMENT_LIMIT
//! 0xC000012D）；释放（decommit）不得大于已提交（MEMORY_NOT_ALLOCATED
//! 0xC00000A0）；零页操作不入账；提交账与占用面对平（净提交==占用）。
//!
//! 结构：提交账（CommitBook：页计数 × 上限 + 操作账）+ 提交/释放双核
//! （四码守卫）+ 对平面 + 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12031_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19600 独占 0x20）。
pub const SEG_BASE: u32 = 0x19600;
pub const MM_COMMIT_LIMIT: u32 = SEG_BASE + 0x01; // 越承诺上限
pub const MM_NOT_ALLOCATED: u32 = SEG_BASE + 0x02; // 释放未提交面
pub const MM_ZERO_PAGE: u32 = SEG_BASE + 0x03; // 零页操作
pub const MM_BAD_REC: u32 = SEG_BASE + 0x04; // 伪造操作账
pub const MM_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 语义钉值。
pub const STATUS_COMMITMENT_LIMIT: u32 = 0xC000_012D;
pub const STATUS_MEMORY_NOT_ALLOCATED: u32 = 0xC000_00A0;

/// 一笔页操作的账目。
#[derive(Clone)]
pub struct MmRec {
    pub seq: u32,
    /// 操作码：1=commit 2=decommit。
    pub op: u8,
    pub pages: u32,
    pub note: String,
}

impl MmRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && self.pages > 0
    }
}

/// 提交账模型。
pub struct CommitBook {
    pub committed: u32,
    pub limit: u32,
    pub ledger: Vec<MmRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl CommitBook {
    pub fn new(limit: u32) -> CommitBook {
        CommitBook { committed: 0, limit, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 提交核：正页数 + 不越上限。
    pub fn commit(&mut self, pages: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MM_FROZEN);
        }
        if note.is_empty() {
            return Err(MM_ZERO_PAGE);
        }
        if pages == 0 {
            return Err(MM_ZERO_PAGE);
        }
        // 相加防回绕：pages > limit-committed 才越。
        if pages > self.limit - self.committed {
            return Err(MM_COMMIT_LIMIT);
        }
        let rec = MmRec { seq: self.seq_next, op: 1, pages, note: note.to_string() };
        if !rec.ok() {
            return Err(MM_BAD_REC);
        }
        self.committed += pages;
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(())
    }

    /// 释放核：不得大于已提交。
    pub fn decommit(&mut self, pages: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MM_FROZEN);
        }
        if note.is_empty() {
            return Err(MM_ZERO_PAGE);
        }
        if pages == 0 {
            return Err(MM_ZERO_PAGE);
        }
        if pages > self.committed {
            return Err(MM_NOT_ALLOCATED);
        }
        let rec = MmRec { seq: self.seq_next, op: 2, pages, note: note.to_string() };
        if !rec.ok() {
            return Err(MM_BAD_REC);
        }
        self.committed -= pages;
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(())
    }

    /// 对平面：净提交 == 占用。
    pub fn tallies(&self) -> bool {
        let mut net: i64 = 0;
        for r in &self.ledger {
            if r.op == 1 {
                net += r.pages as i64;
            } else {
                net -= r.pages as i64;
            }
        }
        net == self.committed as i64
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.committed);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CommitBook::new(0x100);
    // 1) 提交越上限 → MM_COMMIT_LIMIT。
    got[0] = b.commit(0x101, "over").err() == Some(MM_COMMIT_LIMIT);
    // 2) 零页提交 → MM_ZERO_PAGE。
    got[1] = b.commit(0, "z").err() == Some(MM_ZERO_PAGE);
    // 3) 释放未提交面 → MM_NOT_ALLOCATED。
    got[2] = b.decommit(1, "free").err() == Some(MM_NOT_ALLOCATED);
    // 4) 空留痕 → MM_ZERO_PAGE（无痕操作守卫）。
    got[3] = b.commit(1, "").err() == Some(MM_ZERO_PAGE);
    // 5) 印账后动作 → MM_FROZEN。
    let _ = b.commit(0x100, "full");
    b.seal();
    got[4] = b.decommit(1, "post").err() == Some(MM_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12031_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12031");

    // J1 正路径：提交/释放对平。
    let j1 = {
        let mut b = CommitBook::new(0x100);
        let _ = b.commit(0x40, "a");
        let _ = b.commit(0x40, "b");
        let _ = b.decommit(0x20, "c");
        b.committed == 0x60 && b.tallies()
    };
    s.add("J1-提交对平", j1, "0x40+0x40-0x20=0x60");

    // J2 越上限拒。
    let j2 = {
        let mut b = CommitBook::new(0x100);
        b.commit(0x101, "o").err() == Some(MM_COMMIT_LIMIT)
    };
    s.add("J2-越限拒", j2, "超 1 页必拒");

    // J3 恰满边界收。
    let j3 = {
        let mut b = CommitBook::new(0x100);
        b.commit(0x100, "full").is_ok() && b.committed == 0x100
    };
    s.add("J3-恰满边界", j3, "满额合法");

    // J4 满后再提交 1 页拒。
    let j4 = {
        let mut b = CommitBook::new(0x100);
        let _ = b.commit(0x100, "full");
        b.commit(1, "one").err() == Some(MM_COMMIT_LIMIT)
    };
    s.add("J4-满后再扣拒", j4, "上沿面");

    // J5 释放未提交面拒。
    let j5 = {
        let mut b = CommitBook::new(0x100);
        b.decommit(1, "f").err() == Some(MM_NOT_ALLOCATED)
    };
    s.add("J5-空释放拒", j5, "MEMORY_NOT_ALLOCATED");

    // J6 释放越占用拒。
    let j6 = {
        let mut b = CommitBook::new(0x100);
        let _ = b.commit(0x10, "a");
        b.decommit(0x11, "f").err() == Some(MM_NOT_ALLOCATED)
    };
    s.add("J6-超释放拒", j6, "超 1 页必拒");

    // J7 零页操作拒（双面）。
    let j7 = {
        let mut b = CommitBook::new(0x100);
        let c = b.commit(0, "z").err() == Some(MM_ZERO_PAGE);
        let d = b.decommit(0, "z").err() == Some(MM_ZERO_PAGE);
        c && d
    };
    s.add("J7-零页拒", j7, "零页双面拒");

    // J8 账目留痕：op 1/2 逐笔。
    let j8 = {
        let mut b = CommitBook::new(0x100);
        let _ = b.commit(0x10, "a");
        let _ = b.decommit(0x08, "b");
        b.ledger.len() == 2 && b.ledger[0].op == 1 && b.ledger[1].op == 2 && b.ledger_ok()
    };
    s.add("J8-操作留痕", j8, "op=1/2 对账");

    // J9 语义钉值核对。
    let j9 = STATUS_COMMITMENT_LIMIT == 0xC000_012D
        && STATUS_MEMORY_NOT_ALLOCATED == 0xC000_00A0;
    s.add("J9-钉值核对", j9, "0x12D/0x0A0");

    // J10 印账冻结。
    let j10 = {
        let mut b = CommitBook::new(0x100);
        let _ = b.commit(0x10, "a");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2
            && b.commit(0x10, "post").err() == Some(MM_FROZEN)
            && b.decommit(0x10, "post").err() == Some(MM_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：提交压力面。
    let mut b = CommitBook::new(0x400);
    let _ = b.commit(0x100, "s1");
    let _ = b.commit(0x100, "s2");
    let _ = b.decommit(0x80, "s3");
    s.add("样例占用核对", b.committed == 0x180, "0x180 页");
    s.add("样例对平", b.tallies(), "净额=占用");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12031_commit_flow() {
        let mut b = CommitBook::new(0x100);
        assert!(b.commit(0x60, "a").is_ok());
        assert_eq!(b.commit(0xA1, "b"), Err(MM_COMMIT_LIMIT));
        assert_eq!(b.decommit(0x61, "c"), Err(MM_NOT_ALLOCATED));
        assert!(b.decommit(0x60, "d").is_ok());
        assert_eq!(b.committed, 0);
        assert!(b.tallies());
    }

    #[test]
    fn unx12031_checks_green() {
        let s = run_unx12031_checks();
        assert!(s.all_passed(), "UNX-F12031 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12031_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
