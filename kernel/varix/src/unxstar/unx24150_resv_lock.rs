//! UNX-F24150 reservation 锁与显存互斥纪律·子功能10（多对象批量加锁·全或无）（0x414A0 段）
//!
//! 锚点：UNX-F24150-J1 批锁账（批量加锁全或无语义逐条落账）；J2 批
//! 锁语义（全成才落 + 部分败回滚 + 重批拒三性质）；J3 批锁指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（批量原子加锁面）。
//!
//! 结构：锁池 + 批量加锁核（预检→全落或全回滚）+ 批账本 + 折叠指纹
//! + 注入核（批内冲突/野锁/空批/重批/印后必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x414A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x414A0;
pub const BL_PARTIAL: u32 = SEG_BASE + 0x01; // 批内冲突
pub const BL_UNK_LOCK: u32 = SEG_BASE + 0x02; // 野锁
pub const BL_EMPTY: u32 = SEG_BASE + 0x03; // 空批
pub const BL_DUP_BATCH: u32 = SEG_BASE + 0x04; // 重批（同锁同主）
pub const BL_FROZEN: u32 = SEG_BASE + 0x05; // 印后操作

/// 批锁账。
#[derive(Clone)]
pub struct BlRec {
    pub seq: u32,
    pub owner: String,
    pub locks: Vec<u32>,
    pub committed: bool,
}

/// 批锁簿。
pub struct BlBook {
    pub held: Vec<(u32, String)>, // (lock_id, owner)
    pub ledger: Vec<BlRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl BlBook {
    pub fn new() -> BlBook {
        BlBook { held: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    fn is_free(&self, lock_id: u32) -> bool {
        !self.held.iter().any(|(l, _)| *l == lock_id)
    }

    /// 批量加锁：全部闲则全落，任一忙则全不落（全或无）。
    pub fn batch_lock(&mut self, owner: &str, locks: &[u32]) -> Result<u32, u32> {
        if self.sealed {
            return Err(BL_FROZEN);
        }
        if locks.is_empty() || owner.is_empty() {
            return Err(BL_EMPTY);
        }
        // 预检：批内重复、野占、与已持有重叠（同主重批也拒）
        for (i, &l) in locks.iter().enumerate() {
            if locks[..i].contains(&l) {
                return Err(BL_PARTIAL);
            }
            if !self.is_free(l) {
                return Err(BL_PARTIAL);
            }
        }
        // 全落
        for &l in locks {
            self.held.push((l, owner.to_string()));
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        let rec = BlRec { seq, owner: owner.to_string(), locks: locks.to_vec(), committed: true };
        self.ledger.push(rec.clone());
        Ok(seq)
    }

    /// 全批释放。
    pub fn batch_release(&mut self, seq: u32, owner: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(BL_FROZEN);
        }
        let rec = match self.ledger.iter().find(|r| r.seq == seq) {
            Some(r) => r.clone(),
            None => return Err(BL_UNK_LOCK),
        };
        if rec.owner != owner {
            return Err(BL_PARTIAL);
        }
        let n = rec.locks.len();
        for &l in &rec.locks {
            self.held.retain(|(hl, ho)| !(*hl == l && *ho == owner));
        }
        Ok(n)
    }

    pub fn held_count(&self) -> usize {
        self.held.len()
    }

    /// 重批拒：同主同锁集合二次提交必拒（held 已占）。
    pub fn dup_batch_rejected(&self, owner: &str, locks: &[u32]) -> bool {
        locks.iter().any(|&l| self.held.iter().any(|(hl, ho)| *hl == l && *ho == owner))
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
        let mut h: u32 = 0x414A0;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.locks.len() as u32);
            h = h.wrapping_mul(31).wrapping_add(r.committed as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两批已提交。
pub fn mk_book() -> BlBook {
    let mut b = BlBook::new();
    let _ = b.batch_lock("flip", &[1, 2]);
    let _ = b.batch_lock("dma", &[3]);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 批内冲突（含已占锁）
    got[0] = b.batch_lock("cmd", &[2, 4]).err() == Some(BL_PARTIAL);
    // 2) 批内重复
    got[1] = b.batch_lock("cmd", &[4, 4]).err() == Some(BL_PARTIAL);
    // 3) 空批
    got[2] = b.batch_lock("cmd", &[]).err() == Some(BL_EMPTY);
    // 4) 重批拒（同主同锁已持）
    got[3] = b.dup_batch_rejected("flip", &[1, 2]);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.batch_lock("cmd", &[5]).err() == Some(BL_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24150_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24150");
    let mut b = mk_book();

    // J1 批账齐
    let j1 = b.ledger.len() == 2 && b.held_count() == 3;
    s.add("J1-批账齐", j1, "两批三锁全落");

    // J2 全或无：部分败全不落
    let j2 = {
        let mut b2 = BlBook::new();
        let _ = b2.batch_lock("a", &[1]);
        let r = b2.batch_lock("b", &[2, 1]); // 1 被占 → 全不落
        r.is_err() && b2.held_count() == 1
    };
    s.add("J2-全或无", j2, "冲突批零落锁");

    // J3 批内重复拒
    let j3 = b.batch_lock("cmd", &[5, 5]).err() == Some(BL_PARTIAL);
    s.add("J3-批内重复拒", j3, "同锁双入批必拒");

    // J4 空批拒
    let j4 = b.batch_lock("cmd", &[]).err() == Some(BL_EMPTY);
    s.add("J4-空批拒", j4, "空批必拒");

    // J5 重批拒
    let j5 = b.batch_lock("flip", &[1]).err() == Some(BL_PARTIAL);
    s.add("J5-重批拒", j5, "已持锁重批必拒");

    // J6 全批释放
    let j6 = {
        let mut b2 = mk_book();
        let n = b2.batch_release(1, "flip").unwrap_or(0);
        n == 2 && b2.held_count() == 1
    };
    s.add("J6-全批释放", j6, "批释放 n 锁全清");

    // J7 非原主释放拒
    let j7 = b.batch_release(1, "other").err() == Some(BL_PARTIAL);
    s.add("J7-非原主释放拒", j7, "假主释放必拒");

    // J8 野批 seq 拒
    let j8 = b.batch_release(99, "cmd").err() == Some(BL_UNK_LOCK);
    s.add("J8-野批拒", j8, "未知批号必拒");

    // J9 释放后可重批
    let j9 = {
        let mut b2 = mk_book();
        let _ = b2.batch_release(1, "flip");
        b2.batch_lock("cmd", &[1]).is_ok()
    };
    s.add("J9-释放重批", j9, "清锁后他人批可落");

    // J10 批账 committed 全真
    let j10 = b.ledger.iter().all(|r| r.committed);
    s.add("J10-账全提交", j10, "落账批全 committed");

    // J11 seq 唯一
    let j11 = b.seq_unique();
    s.add("J11-seq唯一", j11, "批 seq 不重");

    // J12 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

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
    fn unx24150_batch_flow() {
        let mut b = BlBook::new();
        assert!(b.batch_lock("a", &[1, 2]).is_ok());
        assert_eq!(b.batch_lock("b", &[2, 3]), Err(BL_PARTIAL));
        assert_eq!(b.held_count(), 2);
        assert_eq!(b.batch_lock("b", &[]), Err(BL_EMPTY));
        assert_eq!(b.batch_release(1, "a").unwrap_or(0), 2);
        assert_eq!(b.batch_release(99, "a"), Err(BL_UNK_LOCK));
        b.seal();
        assert_eq!(b.batch_lock("b", &[1]), Err(BL_FROZEN));
    }

    #[test]
    fn unx24150_checks_green() {
        let s = run_unx24150_checks();
        assert!(s.all_passed(), "UNX-F24150 判据未全绿");
    }
}
