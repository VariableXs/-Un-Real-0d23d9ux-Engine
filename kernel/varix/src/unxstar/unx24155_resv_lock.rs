//! UNX-F24155 reservation 锁与显存互斥纪律·子功能15（读写锁·读共享写独占）（0x41540 段）
//!
//! 锚点：UNX-F24155-J1 模式账（读/写模式获取与释放逐条落账）；J2 模
//! 式语义（读计数共享 + 写全清 + 写等读空三性质）；J3 模式指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（读写锁模式面）。
//!
//! 结构：读写锁表 + 读/写获取释放核 + 账本 + 折叠指纹 + 注入核（写
//! 等读/双写/野锁/印后/重复读登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41540 独占 0x20）。
pub const SEG_BASE: u32 = 0x41540;
pub const RW_WRITE_WAIT: u32 = SEG_BASE + 0x01; // 写等读空
pub const RW_DUAL_WRITE: u32 = SEG_BASE + 0x02; // 双写
pub const RW_UNK_LOCK: u32 = SEG_BASE + 0x03; // 野锁
pub const RW_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const RW_BAD_REL: u32 = SEG_BASE + 0x05; // 非法释放

/// 读写锁条目。
#[derive(Clone)]
pub struct RwEnt {
    pub lock_id: u32,
    pub readers: Vec<String>,
    pub writer: Option<String>,
}

/// 读写账。
#[derive(Clone)]
pub struct RwRec {
    pub seq: u32,
    pub lock_id: u32,
    pub who: String,
    pub op: u8, // 0=rdAcq 1=rdRel 2=wrAcq 3=wrRel
}

/// 读写簿。
pub struct RwBook {
    pub locks: Vec<RwEnt>,
    pub ledger: Vec<RwRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl RwBook {
    pub fn new() -> RwBook {
        RwBook { locks: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn declare(&mut self, lock_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(RW_FROZEN);
        }
        if self.locks.iter().any(|l| l.lock_id == lock_id) {
            return Err(RW_UNK_LOCK);
        }
        self.locks.push(RwEnt { lock_id, readers: Vec::new(), writer: None });
        Ok(())
    }

    fn rec(&mut self, lock_id: u32, who: &str, op: u8) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(RwRec { seq, lock_id, who: who.to_string(), op });
    }

    pub fn read_lock(&mut self, lock_id: u32, who: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(RW_FROZEN);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(RW_UNK_LOCK),
        };
        if ent.writer.is_some() {
            return Err(RW_WRITE_WAIT);
        }
        if ent.readers.iter().any(|r| r == who) {
            return Err(RW_BAD_REL);
        }
        ent.readers.push(who.to_string());
        self.rec(lock_id, who, 0);
        Ok(())
    }

    pub fn read_unlock(&mut self, lock_id: u32, who: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(RW_FROZEN);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(RW_UNK_LOCK),
        };
        match ent.readers.iter().position(|r| r == who) {
            Some(i) => {
                ent.readers.remove(i);
                self.rec(lock_id, who, 1);
                Ok(())
            }
            None => Err(RW_BAD_REL),
        }
    }

    pub fn write_lock(&mut self, lock_id: u32, who: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(RW_FROZEN);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(RW_UNK_LOCK),
        };
        if ent.writer.is_some() {
            return Err(RW_DUAL_WRITE);
        }
        if !ent.readers.is_empty() {
            return Err(RW_WRITE_WAIT);
        }
        ent.writer = Some(who.to_string());
        self.rec(lock_id, who, 2);
        Ok(())
    }

    pub fn write_unlock(&mut self, lock_id: u32, who: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(RW_FROZEN);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(RW_UNK_LOCK),
        };
        match &ent.writer {
            Some(w) if w == who => {
                ent.writer = None;
                self.rec(lock_id, who, 3);
                Ok(())
            }
            _ => Err(RW_BAD_REL),
        }
    }

    pub fn state_of(&self, lock_id: u32) -> (usize, bool) {
        match self.locks.iter().find(|l| l.lock_id == lock_id) {
            Some(e) => (e.readers.len(), e.writer.is_some()),
            None => (0, false),
        }
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
        let mut h: u32 = 0x41540;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.lock_id);
            h = h.wrapping_mul(31).wrapping_add(r.op as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两锁一读一写。
pub fn mk_book() -> RwBook {
    let mut b = RwBook::new();
    let _ = b.declare(1);
    let _ = b.declare(2);
    let _ = b.read_lock(1, "r1");
    let _ = b.read_lock(1, "r2");
    let _ = b.write_lock(2, "w1");
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 写等读空
    got[0] = b.write_lock(1, "w2").err() == Some(RW_WRITE_WAIT);
    // 2) 双写
    got[1] = b.write_lock(2, "w3").err() == Some(RW_DUAL_WRITE);
    // 3) 写时读拒
    got[2] = b.read_lock(2, "r9").err() == Some(RW_WRITE_WAIT);
    // 4) 重复读登记
    got[3] = b.read_lock(1, "r1").err() == Some(RW_BAD_REL);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.read_lock(1, "r9").err() == Some(RW_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24155_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24155");
    let mut b = mk_book();

    // J1 锁表齐
    let j1 = b.locks.len() == 2;
    s.add("J1-锁表齐", j1, "两锁全落表");

    // J2 读共享计数
    let j2 = b.state_of(1) == (2, false);
    s.add("J2-读共享", j2, "双读者共存");

    // J3 写独占
    let j3 = b.state_of(2) == (0, true);
    s.add("J3-写独占", j3, "写者零读者");

    // J4 读释放降计数
    let j4 = {
        let _ = b.read_unlock(1, "r1");
        b.state_of(1) == (1, false)
    };
    s.add("J4-读释放降计", j4, "r1 释放后读者 1");

    // J5 写释放清独占
    let j5 = {
        let _ = b.write_unlock(2, "w1");
        b.state_of(2) == (0, false)
    };
    s.add("J5-写释放清", j5, "写释放后全闲");

    // J6 写等读空拒
    let j6 = {
        let mut b2 = mk_book();
        b2.write_lock(1, "w2").err() == Some(RW_WRITE_WAIT)
    };
    s.add("J6-写等读拒", j6, "有读者时写必拒");

    // J7 双写拒
    let j7 = b.write_lock(2, "w3").err() == Some(RW_DUAL_WRITE);
    s.add("J7-双写拒", j7, "已有写者再写必拒");

    // J8 非持有写释放拒
    let j8 = b.write_unlock(2, "other").err() == Some(RW_BAD_REL);
    s.add("J8-非持写释放拒", j8, "假写者释放必拒");

    // J9 非读者读释放拒
    let j9 = b.read_unlock(1, "ghost").err() == Some(RW_BAD_REL);
    s.add("J9-非读者释放拒", j9, "未读而释必拒");

    // J10 野锁拒
    let j10 = b.read_lock(9, "x").err() == Some(RW_UNK_LOCK);
    s.add("J10-野锁拒", j10, "未声明锁必拒");

    // J11 模式账 op 闭集
    let j11 = b.ledger.iter().all(|r| r.op <= 3);
    s.add("J11-op闭集", j11, "操作码不越 0..=3");

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
    fn unx24155_rw_flow() {
        let mut b = RwBook::new();
        let _ = b.declare(1);
        let _ = b.read_lock(1, "a");
        let _ = b.read_lock(1, "b");
        assert_eq!(b.write_lock(1, "w"), Err(RW_WRITE_WAIT));
        assert!(b.read_unlock(1, "a").is_ok());
        assert!(b.read_unlock(1, "b").is_ok());
        assert!(b.write_lock(1, "w").is_ok());
        assert_eq!(b.read_lock(1, "a"), Err(RW_WRITE_WAIT));
        assert!(b.write_unlock(1, "w").is_ok());
        b.seal();
        assert_eq!(b.read_lock(1, "a"), Err(RW_FROZEN));
    }

    #[test]
    fn unx24155_checks_green() {
        let s = run_unx24155_checks();
        assert!(s.all_passed(), "UNX-F24155 判据未全绿");
    }
}
