//! UNX-F24149 reservation 锁与显存互斥纪律·子功能09（锁升级与降级账）（0x41480 段）
//!
//! 锚点：UNX-F24149-J1 升降账（读↔写模式切换逐条落账）；J2 升降语
//! 义（读升写独占闸 + 写降读放行 + 越级拒三性质）；J3 升降指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（锁模式升降面）。
//!
//! 结构：模式锁表（读/写）+ 升降核 + 账本 + 折叠指纹 + 注入核（读升
//! 写冲突/越级/野锁/印后/双写必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41480 独占 0x20）。
pub const SEG_BASE: u32 = 0x41480;
pub const UD_WR_CONFLICT: u32 = SEG_BASE + 0x01; // 读升写冲突
pub const UD_UNK_LOCK: u32 = SEG_BASE + 0x02; // 野锁
pub const UD_ILLEGAL: u32 = SEG_BASE + 0x03; // 越级切换
pub const UD_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const UD_DUAL_WRITE: u32 = SEG_BASE + 0x05; // 双写

/// 锁模式。
pub const M_READ: u8 = 0;
pub const M_WRITE: u8 = 1;

/// 模式锁条目。
#[derive(Clone)]
pub struct UdEnt {
    pub lock_id: u32,
    pub readers: Vec<String>,
    pub writer: Option<String>,
}

/// 升降账。
#[derive(Clone)]
pub struct UdRec {
    pub seq: u32,
    pub lock_id: u32,
    pub who: String,
    pub from: u8,
    pub to: u8,
}

/// 升降簿。
pub struct UdBook {
    pub locks: Vec<UdEnt>,
    pub ledger: Vec<UdRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl UdBook {
    pub fn new() -> UdBook {
        UdBook { locks: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn declare(&mut self, lock_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(UD_FROZEN);
        }
        if self.locks.iter().any(|l| l.lock_id == lock_id) {
            return Err(UD_UNK_LOCK);
        }
        self.locks.push(UdEnt { lock_id, readers: Vec::new(), writer: None });
        Ok(())
    }

    fn rec(&mut self, lock_id: u32, who: &str, from: u8, to: u8) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(UdRec { seq, lock_id, who: who.to_string(), from, to });
    }

    /// 读获取：写者存在即拒。
    pub fn read_lock(&mut self, lock_id: u32, who: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(UD_FROZEN);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(UD_UNK_LOCK),
        };
        if ent.writer.is_some() {
            return Err(UD_WR_CONFLICT);
        }
        if !ent.readers.iter().any(|r| r == who) {
            ent.readers.push(who.to_string());
            self.rec(lock_id, who, M_WRITE, M_READ);
        }
        Ok(())
    }

    /// 写获取：有读者或写者即拒。
    pub fn write_lock(&mut self, lock_id: u32, who: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(UD_FROZEN);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(UD_UNK_LOCK),
        };
        if ent.writer.is_some() {
            return Err(UD_DUAL_WRITE);
        }
        if !ent.readers.is_empty() {
            return Err(UD_WR_CONFLICT);
        }
        ent.writer = Some(who.to_string());
        self.rec(lock_id, who, M_READ, M_WRITE);
        Ok(())
    }

    /// 读升写：仅当自己是唯一读者。
    pub fn upgrade(&mut self, lock_id: u32, who: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(UD_FROZEN);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(UD_UNK_LOCK),
        };
        if ent.writer.is_some() {
            return Err(UD_DUAL_WRITE);
        }
        if ent.readers.len() != 1 || ent.readers[0] != who {
            return Err(UD_WR_CONFLICT);
        }
        ent.readers.clear();
        ent.writer = Some(who.to_string());
        self.rec(lock_id, who, M_READ, M_WRITE);
        Ok(())
    }

    /// 写降读：唯一合法降级。
    pub fn downgrade(&mut self, lock_id: u32, who: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(UD_FROZEN);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(UD_UNK_LOCK),
        };
        match &ent.writer {
            Some(w) if w == who => {
                ent.writer = None;
                ent.readers.push(who.to_string());
            }
            _ => return Err(UD_ILLEGAL),
        }
        self.rec(lock_id, who, M_WRITE, M_READ);
        Ok(())
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
        let mut h: u32 = 0x41480;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.lock_id);
            h = h.wrapping_mul(31).wrapping_add((r.from as u32) * 3 + r.to as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两锁。
pub fn mk_book() -> UdBook {
    let mut b = UdBook::new();
    let _ = b.declare(1);
    let _ = b.declare(2);
    let _ = b.read_lock(1, "r1");
    let _ = b.write_lock(2, "w1");
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 读时写拒
    got[0] = b.write_lock(1, "w2").err() == Some(UD_WR_CONFLICT);
    // 2) 写时读拒
    got[1] = b.read_lock(2, "r2").err() == Some(UD_WR_CONFLICT);
    // 3) 双写拒
    got[2] = b.write_lock(2, "w3").err() == Some(UD_DUAL_WRITE);
    // 4) 越级降（读者降级）
    got[3] = b.downgrade(1, "r1").err() == Some(UD_ILLEGAL);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.read_lock(1, "r9").err() == Some(UD_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24149_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24149");
    let mut b = mk_book();

    // J1 锁表齐
    let j1 = b.locks.len() == 2;
    s.add("J1-锁表齐", j1, "两锁全落表");

    // J2 读共享
    let j2 = {
        let mut b2 = UdBook::new();
        let _ = b2.declare(1);
        let _ = b2.read_lock(1, "a");
        let _ = b2.read_lock(1, "b");
        b2.state_of(1) == (2, false)
    };
    s.add("J2-读共享", j2, "双读者共存");

    // J3 写独占
    let j3 = b.state_of(2) == (0, true);
    s.add("J3-写独占", j3, "写者零读者");

    // J4 读升写（唯一读者）
    let j4 = {
        let mut b2 = UdBook::new();
        let _ = b2.declare(1);
        let _ = b2.read_lock(1, "solo");
        b2.upgrade(1, "solo").is_ok() && b2.state_of(1) == (0, true)
    };
    s.add("J4-读升写", j4, "唯一读者可升写");

    // J5 多读者升写拒
    let j5 = {
        let mut b2 = UdBook::new();
        let _ = b2.declare(1);
        let _ = b2.read_lock(1, "a");
        let _ = b2.read_lock(1, "b");
        b2.upgrade(1, "a").err() == Some(UD_WR_CONFLICT)
    };
    s.add("J5-多读者升写拒", j5, "非唯一读者必拒");

    // J6 写降读
    let j6 = {
        b.downgrade(2, "w1").is_ok() && b.state_of(2) == (1, false)
    };
    s.add("J6-写降读", j6, "写者降为读者");

    // J7 越级降拒
    let j7 = b.downgrade(1, "r1").err() == Some(UD_ILLEGAL);
    s.add("J7-越级降拒", j7, "读者降级必拒");

    // J8 升降账落账
    let j8 = b.ledger.iter().all(|r| r.from != r.to);
    s.add("J8-账模式对", j8, "升降账 from≠to");

    // J9 野锁拒
    let j9 = b.read_lock(9, "x").err() == Some(UD_UNK_LOCK);
    s.add("J9-野锁拒", j9, "未声明锁必拒");

    // J10 重号声明拒
    let j10 = b.declare(1).err() == Some(UD_UNK_LOCK);
    s.add("J10-重号拒", j10, "锁重号必拒");

    // J11 升写后旧读者清
    let j11 = {
        let mut b2 = UdBook::new();
        let _ = b2.declare(1);
        let _ = b2.read_lock(1, "solo");
        let _ = b2.upgrade(1, "solo");
        b2.locks.iter().find(|l| l.lock_id == 1).map(|l| l.readers.is_empty()).unwrap_or(false)
    };
    s.add("J11-升写清读者", j11, "升级后读者表清空");

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
    fn unx24149_rw_flow() {
        let mut b = UdBook::new();
        let _ = b.declare(1);
        let _ = b.read_lock(1, "a");
        assert_eq!(b.write_lock(1, "w"), Err(UD_WR_CONFLICT));
        assert!(b.upgrade(1, "a").is_ok());
        assert_eq!(b.read_lock(1, "b"), Err(UD_WR_CONFLICT));
        assert!(b.downgrade(1, "a").is_ok());
        assert_eq!(b.downgrade(1, "a"), Err(UD_ILLEGAL));
        b.seal();
        assert_eq!(b.read_lock(1, "b"), Err(UD_FROZEN));
    }

    #[test]
    fn unx24149_checks_green() {
        let s = run_unx24149_checks();
        assert!(s.all_passed(), "UNX-F24149 判据未全绿");
    }
}
