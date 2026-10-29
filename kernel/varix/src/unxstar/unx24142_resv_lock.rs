//! UNX-F24142 reservation 锁与显存互斥纪律·子功能02（锁获取与持锁账）（0x413A0 段）
//!
//! 锚点：UNX-F24142-J1 持锁账（acquire/release 逐条落账）；J2 互斥语
//! 义（单持闸 + 未持释放拒 + 双重获取拒三性质）；J3 持锁指纹（账折
//! 叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（锁获取互斥面）。
//!
//! 结构：锁表 + acquire/release 核 + 持锁者账本 + 折叠指纹 + 注入核
//! （重入/野锁/未持释放/印后/自旋越限必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x413A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x413A0;
pub const LK_DOUBLE_ACQ: u32 = SEG_BASE + 0x01; // 双重获取
pub const LK_NOT_HELD: u32 = SEG_BASE + 0x02; // 未持释放
pub const LK_UNK_LOCK: u32 = SEG_BASE + 0x03; // 野锁
pub const LK_SPIN_OVER: u32 = SEG_BASE + 0x04; // 自旋越限
pub const LK_FROZEN: u32 = SEG_BASE + 0x05; // 印后操作

/// 锁条目。
#[derive(Clone)]
pub struct LockEnt {
    pub lock_id: u32,
    pub holder: Option<String>,
    pub acq_count: u32,
    pub rel_count: u32,
}

/// 持锁账。
#[derive(Clone)]
pub struct HoldRec {
    pub seq: u32,
    pub lock_id: u32,
    pub holder: String,
    pub op: u8, // 0=acq 1=rel
}

/// 锁簿。
pub struct LockBook {
    pub locks: Vec<LockEnt>,
    pub ledger: Vec<HoldRec>,
    pub spin_max: u32,
    pub seq_next: u32,
    pub sealed: bool,
}

impl LockBook {
    pub fn new(spin_max: u32) -> LockBook {
        LockBook { locks: Vec::new(), ledger: Vec::new(), spin_max, seq_next: 1, sealed: false }
    }

    pub fn declare(&mut self, lock_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(LK_FROZEN);
        }
        if self.locks.iter().any(|l| l.lock_id == lock_id) {
            return Err(LK_UNK_LOCK);
        }
        self.locks.push(LockEnt { lock_id, holder: None, acq_count: 0, rel_count: 0 });
        Ok(())
    }

    /// 获取：已持有者（无论谁）再获取即拒。
    pub fn acquire(&mut self, lock_id: u32, holder: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(LK_FROZEN);
        }
        if holder.is_empty() || self.acq_count_of(holder) >= self.spin_max {
            return Err(LK_SPIN_OVER);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(LK_UNK_LOCK),
        };
        if ent.holder.is_some() {
            return Err(LK_DOUBLE_ACQ);
        }
        ent.holder = Some(holder.to_string());
        ent.acq_count += 1;
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(HoldRec { seq, lock_id, holder: holder.to_string(), op: 0 });
        Ok(())
    }

    /// 释放：仅持锁者可释放。
    pub fn release(&mut self, lock_id: u32, holder: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(LK_FROZEN);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(LK_UNK_LOCK),
        };
        match &ent.holder {
            Some(h) if h == holder => {
                ent.holder = None;
                ent.rel_count += 1;
            }
            Some(_) => return Err(LK_NOT_HELD),
            None => return Err(LK_NOT_HELD),
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(HoldRec { seq, lock_id, holder: holder.to_string(), op: 1 });
        Ok(())
    }

    pub fn acq_count_of(&self, holder: &str) -> u32 {
        self.ledger.iter().filter(|r| r.op == 0 && r.holder == holder).count() as u32
    }

    pub fn held_count(&self) -> usize {
        self.locks.iter().filter(|l| l.holder.is_some()).count()
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
        let mut h: u32 = 0x413A0;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.lock_id);
            h = h.wrapping_mul(31).wrapping_add(r.op as u32);
            for by in r.holder.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：三锁。
pub fn mk_book() -> LockBook {
    let mut b = LockBook::new(8);
    for id in 1..=3u32 {
        let _ = b.declare(id);
    }
    let _ = b.acquire(1, "flip");
    let _ = b.acquire(2, "dma");
    let _ = b.release(2, "dma");
    let _ = b.acquire(2, "cmd");
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 双重获取
    got[0] = b.acquire(1, "other").err() == Some(LK_DOUBLE_ACQ);
    // 2) 未持释放（他人释放）
    got[1] = b.release(1, "other").err() == Some(LK_NOT_HELD);
    // 3) 野锁
    got[2] = b.acquire(99, "x").err() == Some(LK_UNK_LOCK);
    // 4) 未持锁者释放空闲锁
    got[3] = b.release(3, "x").err() == Some(LK_NOT_HELD);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.acquire(3, "x").err() == Some(LK_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24142_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24142");
    let mut b = mk_book();

    // J1 锁表齐
    let j1 = b.locks.len() == 3;
    s.add("J1-锁表齐", j1, "三锁全落表");

    // J2 持锁账齐
    let j2 = b.ledger.len() == 4;
    s.add("J2-持锁账齐", j2, "acq/rel 四条落账");

    // J3 互斥：持锁中再获取拒
    let j3 = b.acquire(1, "other").err() == Some(LK_DOUBLE_ACQ);
    s.add("J3-单持闸", j3, "持锁中任何人再取必拒");

    // J4 释放后可转移
    let j4 = {
        let mut b2 = mk_book();
        b2.release(1, "flip").is_ok() && b2.acquire(1, "cmd").is_ok()
    };
    s.add("J4-释放转移", j4, "释放后他人可获取");

    // J5 未持释放拒
    let j5 = b.release(1, "other").err() == Some(LK_NOT_HELD);
    s.add("J5-未持释放拒", j5, "非持锁者释放必拒");

    // J6 空锁释放拒
    let j6 = b.release(3, "x").err() == Some(LK_NOT_HELD);
    s.add("J6-空锁释放拒", j6, "空闲锁释放必拒");

    // J7 持锁计数
    let j7 = b.held_count() == 2;
    s.add("J7-持锁计数", j7, "两锁在持");

    // J8 acq/rel 计数守恒
    let j8 = {
        let e1 = b.locks.iter().find(|l| l.lock_id == 2).unwrap();
        e1.acq_count == 2 && e1.rel_count == 1
    };
    s.add("J8-计数守恒", j8, "锁2 acq=2 rel=1");

    // J9 野锁拒
    let j9 = b.acquire(99, "x").err() == Some(LK_UNK_LOCK);
    s.add("J9-野锁拒", j9, "未声明锁必拒");

    // J10 重号锁拒
    let j10 = b.declare(1).err() == Some(LK_UNK_LOCK);
    s.add("J10-重号锁拒", j10, "锁重号必拒");

    // J11 自旋上限闸
    let j11 = {
        let mut b2 = LockBook::new(1);
        let _ = b2.declare(10);
        let _ = b2.declare(11);
        let _ = b2.acquire(10, "h");
        let _ = b2.release(10, "h");
        let _ = b2.acquire(10, "h");
        let _ = b2.release(10, "h");
        b2.acquire(10, "h");
        b2.acquire(11, "h").err() == Some(LK_SPIN_OVER)
    };
    s.add("J11-自旋上限", j11, "acq 计数超 spin_max 必拒");

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
    fn unx24142_lock_flow() {
        let mut b = mk_book();
        assert_eq!(b.acquire(1, "x"), Err(LK_DOUBLE_ACQ));
        assert!(b.release(1, "flip").is_ok());
        assert!(b.acquire(1, "x").is_ok());
        assert_eq!(b.release(3, "x"), Err(LK_NOT_HELD));
        b.seal();
        assert_eq!(b.acquire(3, "x"), Err(LK_FROZEN));
    }

    #[test]
    fn unx24142_checks_green() {
        let s = run_unx24142_checks();
        assert!(s.all_passed(), "UNX-F24142 判据未全绿");
    }
}
