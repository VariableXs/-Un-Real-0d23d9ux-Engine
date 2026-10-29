//! UNX-F24145 reservation 锁与显存互斥纪律·子功能05（try-acquire 失败立即返）（0x41400 段）
//!
//! 锚点：UNX-F24145-J1 尝试账（try-acquire 成败逐条落账）；J2 尝试语
//! 义（忙即返 False + 不入等待 + 成败皆落账三性质）；J3 尝试指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（try-acquire 非阻塞面）。
//!
//! 结构：锁态表 + try_lock 核 + 尝试账本 + 折叠指纹 + 注入核（野锁/
//! 忙占成功伪账/重复解锁/印后/零超时阻塞档必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41400 独占 0x20）。
pub const SEG_BASE: u32 = 0x41400;
pub const TA_UNK_LOCK: u32 = SEG_BASE + 0x01; // 野锁
pub const TA_FAKE_SUCCESS: u32 = SEG_BASE + 0x02; // 忙占伪成功
pub const TA_REL_UNOWNED: u32 = SEG_BASE + 0x03; // 重复解锁
pub const TA_FROZEN: u32 = SEG_BASE + 0x04; // 印后尝试
pub const TA_BLOCK_MODE: u32 = SEG_BASE + 0x05; // 阻塞档禁用

/// 锁态。
#[derive(Clone)]
pub struct TaEnt {
    pub lock_id: u32,
    pub busy: bool,
    pub owner: Option<String>,
}

/// 尝试账。
#[derive(Clone)]
pub struct TaRec {
    pub seq: u32,
    pub lock_id: u32,
    pub who: String,
    pub got: bool,
}

/// 尝试簿。
pub struct TaBook {
    pub locks: Vec<TaEnt>,
    pub ledger: Vec<TaRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TaBook {
    pub fn new() -> TaBook {
        TaBook { locks: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn declare(&mut self, lock_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TA_FROZEN);
        }
        if self.locks.iter().any(|l| l.lock_id == lock_id) {
            return Err(TA_UNK_LOCK);
        }
        self.locks.push(TaEnt { lock_id, busy: false, owner: None });
        Ok(())
    }

    /// try-lock：忙即返 Ok(false)（非阻塞），闲则占返 Ok(true)。成败皆落账。
    pub fn try_lock(&mut self, lock_id: u32, who: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(TA_FROZEN);
        }
        if who.is_empty() {
            return Err(TA_BLOCK_MODE);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(TA_UNK_LOCK),
        };
        let got = !ent.busy;
        if got {
            ent.busy = true;
            ent.owner = Some(who.to_string());
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(TaRec { seq, lock_id, who: who.to_string(), got });
        Ok(got)
    }

    /// unlock：仅持有者。
    pub fn unlock(&mut self, lock_id: u32, who: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TA_FROZEN);
        }
        let ent = match self.locks.iter_mut().find(|l| l.lock_id == lock_id) {
            Some(e) => e,
            None => return Err(TA_UNK_LOCK),
        };
        match &ent.owner {
            Some(o) if o == who => {
                ent.busy = false;
                ent.owner = None;
                Ok(())
            }
            _ => Err(TA_REL_UNOWNED),
        }
    }

    pub fn fail_count(&self, lock_id: u32) -> usize {
        self.ledger.iter().filter(|r| r.lock_id == lock_id && !r.got).count()
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
        let mut h: u32 = 0x41400;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.lock_id);
            h = h.wrapping_mul(31).wrapping_add(r.got as u32);
            for by in r.who.bytes() {
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

/// 标准样本：两锁一忙一闲。
pub fn mk_book() -> TaBook {
    let mut b = TaBook::new();
    let _ = b.declare(1);
    let _ = b.declare(2);
    let _ = b.try_lock(1, "flip"); // true
    let _ = b.try_lock(1, "dma"); // false（忙）
    let _ = b.try_lock(2, "dma"); // true
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野锁
    got[0] = b.try_lock(9, "x").err() == Some(TA_UNK_LOCK);
    // 2) 忙锁再试必 false（伪成功被识破）
    got[1] = b.try_lock(1, "cmd") == Ok(false);
    // 3) 重复解锁
    got[2] = b.unlock(1, "cmd").err() == Some(TA_REL_UNOWNED);
    // 4) 空尝试者（阻塞档禁用）
    got[3] = b.try_lock(2, "").err() == Some(TA_BLOCK_MODE);
    // 5) 印后尝试必抓
    b.seal();
    got[4] = b.try_lock(2, "x").err() == Some(TA_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24145_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24145");
    let mut b = mk_book();

    // J1 锁表齐
    let j1 = b.locks.len() == 2;
    s.add("J1-锁表齐", j1, "两锁全落表");

    // J2 忙即返 false 不阻塞
    let j2 = b.try_lock(1, "cmd") == Ok(false);
    s.add("J2-忙即返假", j2, "忙锁 try 立即 false");

    // J3 闲即占
    let j3 = {
        let mut b2 = TaBook::new();
        let _ = b2.declare(7);
        b2.try_lock(7, "a") == Ok(true)
    };
    s.add("J3-闲即占", j3, "闲锁 try 成功占用");

    // J4 成败皆落账
    let j4 = b.ledger.len() >= 4 && b.ledger.iter().any(|r| !r.got);
    s.add("J4-成败皆账", j4, "失败尝试也落账");

    // J5 失败计数
    let j5 = b.fail_count(1) >= 2;
    s.add("J5-失败计数", j5, "锁1 至少两次失败尝试");

    // J6 重复解锁拒
    let j6 = b.unlock(1, "cmd").err() == Some(TA_REL_UNOWNED);
    s.add("J6-重复解锁拒", j6, "非持有者解锁必拒");

    // J7 解锁后可再占
    let j7 = {
        let mut b2 = mk_book();
        let _ = b2.unlock(1, "flip");
        b2.try_lock(1, "cmd") == Ok(true)
    };
    s.add("J7-解锁再占", j7, "释放后他人 try 成功");

    // J8 空尝试者拒
    let j8 = b.try_lock(2, "").err() == Some(TA_BLOCK_MODE);
    s.add("J8-空尝试拒", j8, "空名尝试必拒");

    // J9 野锁拒
    let j9 = b.try_lock(9, "x").err() == Some(TA_UNK_LOCK);
    s.add("J9-野锁拒", j9, "未声明锁必拒");

    // J10 占有者登记
    let j10 = b
        .locks
        .iter()
        .find(|l| l.lock_id == 2)
        .map(|l| l.owner.as_deref() == Some("dma") && l.busy)
        .unwrap_or(false);
    s.add("J10-占有登记", j10, "锁2 持有者 dma");

    // J11 重号声明拒
    let j11 = b.declare(1).err() == Some(TA_UNK_LOCK);
    s.add("J11-重号拒", j11, "锁重号必拒");

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
    fn unx24145_trylock_flow() {
        let mut b = mk_book();
        assert_eq!(b.try_lock(1, "cmd"), Ok(false));
        assert!(b.unlock(1, "flip").is_ok());
        assert_eq!(b.try_lock(1, "cmd"), Ok(true));
        assert_eq!(b.unlock(1, "cmd"), Ok(()));
        assert_eq!(b.unlock(1, "cmd"), Err(TA_REL_UNOWNED));
        b.seal();
        assert_eq!(b.try_lock(1, "cmd"), Err(TA_FROZEN));
    }

    #[test]
    fn unx24145_checks_green() {
        let s = run_unx24145_checks();
        assert!(s.all_passed(), "UNX-F24145 判据未全绿");
    }
}
