//! UNX-F24144 reservation 锁与显存互斥纪律·子功能04（重入账与递归持锁）（0x413E0 段）
//!
//! 锚点：UNX-F24144-J1 重入账（同者递归持锁深度逐条落账）；J2 重入
//! 语义（同者重入放行 + 异者拒 + 深度归零三性质）；J3 重入指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（重入递归持锁面）。
//!
//! 结构：重入锁表 + 递归 acquire/release 核 + 深度账本 + 折叠指纹 +
//! 注入核（异者重入/野锁/超深/印后/他人代释必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x413E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x413E0;
pub const RE_FOREIGN: u32 = SEG_BASE + 0x01; // 异者重入
pub const RE_UNK_LOCK: u32 = SEG_BASE + 0x02; // 野锁
pub const RE_OVER_DEEP: u32 = SEG_BASE + 0x03; // 超深度
pub const RE_FOREIGN_REL: u32 = SEG_BASE + 0x04; // 他人代释
pub const RE_FROZEN: u32 = SEG_BASE + 0x05; // 印后操作

/// 重入锁条目。
#[derive(Clone)]
pub struct ReEnt {
    pub lock_id: u32,
    pub owner: Option<String>,
    pub depth: u32,
    pub deep_max: u32,
}

/// 重入账。
#[derive(Clone)]
pub struct ReRec {
    pub seq: u32,
    pub lock_id: u32,
    pub owner: String,
    pub op: u8, // 0=acq 1=rel
    pub depth_after: u32,
}

/// 重入簿。
pub struct ReBook {
    pub locks: Vec<ReEnt>,
    pub ledger: Vec<ReRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ReBook {
    pub fn new() -> ReBook {
        ReBook { locks: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn declare(&mut self, lock_id: u32, deep_max: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RE_FROZEN);
        }
        if self.locks.iter().any(|l| l.lock_id == lock_id) {
            return Err(RE_UNK_LOCK);
        }
        self.locks.push(ReEnt { lock_id, owner: None, depth: 0, deep_max });
        Ok(())
    }

    /// 递归获取：同者深度+1；异者拒；超深拒。
    pub fn acquire(&mut self, lock_id: u32, owner: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RE_FROZEN);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(RE_UNK_LOCK),
        };
        match &ent.owner {
            Some(o) if o == owner => {
                if ent.depth >= ent.deep_max {
                    return Err(RE_OVER_DEEP);
                }
                ent.depth += 1;
            }
            Some(_) => return Err(RE_FOREIGN),
            None => {
                ent.owner = Some(owner.to_string());
                ent.depth = 1;
            }
        }
        let d = ent.depth;
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(ReRec { seq, lock_id, owner: owner.to_string(), op: 0, depth_after: d });
        Ok(d)
    }

    /// 递归释放：仅持有者，深度-1 至 0 清 owner。
    pub fn release(&mut self, lock_id: u32, owner: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(RE_FROZEN);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(RE_UNK_LOCK),
        };
        match &ent.owner {
            Some(o) if o == owner => {
                ent.depth -= 1;
                if ent.depth == 0 {
                    ent.owner = None;
                }
            }
            Some(_) => return Err(RE_FOREIGN_REL),
            None => return Err(RE_FOREIGN_REL),
        }
        let d = ent.depth;
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(ReRec { seq, lock_id, owner: owner.to_string(), op: 1, depth_after: d });
        Ok(d)
    }

    pub fn depth_of(&self, lock_id: u32) -> u32 {
        self.locks.iter().find(|l| l.lock_id == lock_id).map(|l| l.depth).unwrap_or(0)
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
        let mut h: u32 = 0x413E0;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.lock_id);
            h = h.wrapping_mul(31).wrapping_add(r.op as u32);
            h = h.wrapping_mul(31).wrapping_add(r.depth_after);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两锁（深限 3/1）。
pub fn mk_book() -> ReBook {
    let mut b = ReBook::new();
    let _ = b.declare(1, 3);
    let _ = b.declare(2, 1);
    let _ = b.acquire(1, "gpu");
    let _ = b.acquire(1, "gpu");
    let _ = b.acquire(2, "flip");
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 异者重入
    got[0] = b.acquire(1, "other").err() == Some(RE_FOREIGN);
    // 2) 野锁
    got[1] = b.acquire(9, "x").err() == Some(RE_UNK_LOCK);
    // 3) 超深度
    let _ = b.acquire(1, "gpu"); // depth 3 = deep_max
    got[2] = b.acquire(1, "gpu").err() == Some(RE_OVER_DEEP);
    // 4) 他人代释
    got[3] = b.release(2, "other").err() == Some(RE_FOREIGN_REL);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.acquire(2, "flip").err() == Some(RE_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24144_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24144");
    let mut b = mk_book();

    // J1 锁表齐
    let j1 = b.locks.len() == 2;
    s.add("J1-锁表齐", j1, "两锁全落表");

    // J2 同者重入放行
    let j2 = b.depth_of(1) == 2;
    s.add("J2-同者重入", j2, "gpu 二次获取深度 2");

    // J3 异者拒
    let j3 = b.acquire(1, "other").err() == Some(RE_FOREIGN);
    s.add("J3-异者拒", j3, "非持有者获取必拒");

    // J4 深度上限闸
    let j4 = {
        let mut b2 = ReBook::new();
        let _ = b2.declare(1, 2);
        let _ = b2.acquire(1, "a");
        let _ = b2.acquire(1, "a");
        b2.acquire(1, "a").err() == Some(RE_OVER_DEEP)
    };
    s.add("J4-深度上限", j4, "超 deep_max 必拒");

    // J5 递归释放降深
    let j5 = {
        let d = b.release(1, "gpu").unwrap_or(0);
        d == 1 && b.depth_of(1) == 1
    };
    s.add("J5-递归释放", j5, "释放降深不减 owner");

    // J6 归零清 owner
    let j6 = {
        let _ = b.release(1, "gpu");
        let _ = b.release(1, "gpu");
        b.depth_of(1) == 0
            && b.locks.iter().find(|l| l.lock_id == 1).map(|l| l.owner.is_none()).unwrap_or(false)
    };
    s.add("J6-归零清owner", j6, "深度归零释放持有");

    // J7 他人代释拒
    let j7 = b.release(2, "other").err() == Some(RE_FOREIGN_REL);
    s.add("J7-代释拒", j7, "非持有者释放必拒");

    // J8 重入账落账
    let j8 = b.ledger.iter().all(|r| r.depth_after <= 3);
    s.add("J8-账深守恒", j8, "账面深度不超限");

    // J9 野锁拒
    let j9 = b.acquire(9, "x").err() == Some(RE_UNK_LOCK);
    s.add("J9-野锁拒", j9, "未声明锁必拒");

    // J10 重号声明拒
    let j10 = b.declare(1, 2).err() == Some(RE_UNK_LOCK);
    s.add("J10-重号拒", j10, "锁重号必拒");

    // J11 释放后可转让
    let j11 = {
        let mut b2 = ReBook::new();
        let _ = b2.declare(5, 2);
        let _ = b2.acquire(5, "a");
        let _ = b2.release(5, "a");
        b2.acquire(5, "b").is_ok()
    };
    s.add("J11-释放转让", j11, "归零后他人可持有");

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
    fn unx24144_reentry_flow() {
        let mut b = ReBook::new();
        let _ = b.declare(1, 2);
        assert_eq!(b.acquire(1, "a"), Ok(1));
        assert_eq!(b.acquire(1, "a"), Ok(2));
        assert_eq!(b.acquire(1, "a"), Err(RE_OVER_DEEP));
        assert_eq!(b.acquire(1, "b"), Err(RE_FOREIGN));
        assert_eq!(b.release(1, "b"), Err(RE_FOREIGN_REL));
        assert_eq!(b.release(1, "a"), Ok(1));
        assert_eq!(b.release(1, "a"), Ok(0));
        assert_eq!(b.acquire(1, "b"), Ok(1));
        b.seal();
        assert_eq!(b.acquire(1, "b"), Err(RE_FROZEN));
    }

    #[test]
    fn unx24144_checks_green() {
        let s = run_unx24144_checks();
        assert!(s.all_passed(), "UNX-F24144 判据未全绿");
    }
}
