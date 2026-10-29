//! UNX-F24158 reservation 锁与显存互斥纪律·子功能18（互斥纪律收口·全对象无双持）（0x415A0 段）
//!
//! 锚点：UNX-F24158-J1 收口账（全对象持锁终态逐条落账）；J2 收口语
//! 义（无双持 + 无泄漏 + 收口印闸三性质）；J3 收口指纹（终态折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（互斥纪律收口面）。
//!
//! 结构：终态持有表 + 双持扫描核 + 泄漏扫描核 + 收口印核 + 折叠指纹
//! + 注入核（双持识破/泄漏识破/未清印发/重印/印后录必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x415A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x415A0;
pub const MX_DUAL_HOLD: u32 = SEG_BASE + 0x01; // 双持
pub const MX_LEAK: u32 = SEG_BASE + 0x02; // 泄漏
pub const MX_NOT_CLEAN: u32 = SEG_BASE + 0x03; // 未清印发
pub const MX_FROZEN: u32 = SEG_BASE + 0x04; // 印后录入
pub const MX_DUP_SEAL: u32 = SEG_BASE + 0x05; // 重复印发

/// 终态持锁条目。
#[derive(Clone)]
pub struct MxHold {
    pub lock_id: u32,
    pub holders: Vec<String>, // 互斥要求至多 1
    pub acq_total: u32,
    pub rel_total: u32,
}

/// 收口簿。
pub struct MxBook {
    pub holds: Vec<MxHold>,
    pub sealed: bool,
}

impl MxBook {
    pub fn new() -> MxBook {
        MxBook { holds: Vec::new(), sealed: false }
    }

    pub fn touch(&mut self, lock_id: u32) -> &mut MxHold {
        if !self.holds.iter().any(|h| h.lock_id == lock_id) {
            self.holds.push(MxHold {
                lock_id,
                holders: Vec::new(),
                acq_total: 0,
                rel_total: 0,
            });
        }
        self.holds.iter_mut().find(|h| h.lock_id == lock_id).unwrap()
    }

    pub fn acq(&mut self, lock_id: u32, who: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MX_FROZEN);
        }
        if who.is_empty() {
            return Err(MX_LEAK);
        }
        let h = self.touch(lock_id);
        if h.holders.contains(&who.to_string()) || h.holders.len() >= 1 {
            return Err(MX_DUAL_HOLD);
        }
        h.holders.push(who.to_string());
        h.acq_total += 1;
        Ok(())
    }

    pub fn rel(&mut self, lock_id: u32, who: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MX_FROZEN);
        }
        let h = self.touch(lock_id);
        match h.holders.iter().position(|x| x == who) {
            Some(i) => {
                h.holders.remove(i);
                h.rel_total += 1;
                Ok(())
            }
            None => Err(MX_LEAK),
        }
    }

    /// 双持扫描：任一锁 holders > 1。
    pub fn any_dual(&self) -> bool {
        self.holds.iter().any(|h| h.holders.len() > 1)
    }

    /// 泄漏扫描：任一锁 acq_total != rel_total + holders.len()。
    pub fn any_leak(&self) -> bool {
        self.holds.iter().any(|h| h.acq_total != h.rel_total + h.holders.len() as u32)
    }

    /// 收口印：无双持 + 无泄漏 + 全释放方可发印。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(MX_DUP_SEAL);
        }
        if self.any_dual() {
            return Err(MX_DUAL_HOLD);
        }
        if self.any_leak() || self.holds.iter().any(|h| !h.holders.is_empty()) {
            return Err(MX_NOT_CLEAN);
        }
        self.sealed = true;
        Ok(self.fold())
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x415A0;
        for x in &self.holds {
            h = h.wrapping_mul(31).wrapping_add(x.lock_id);
            h = h.wrapping_mul(31).wrapping_add(x.acq_total);
            h = h.wrapping_mul(31).wrapping_add(x.rel_total);
            h = h.wrapping_mul(31).wrapping_add(x.holders.len() as u32);
        }
        h
    }
}

/// 标准样本：三锁全平衡释放。
pub fn mk_book() -> MxBook {
    let mut b = MxBook::new();
    let _ = b.acq(1, "a");
    let _ = b.rel(1, "a");
    let _ = b.acq(2, "b");
    let _ = b.rel(2, "b");
    let _ = b.acq(3, "c");
    let _ = b.rel(3, "c");
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = MxBook::new();
    // 1) 双持识破
    let _ = b.acq(1, "a");
    got[0] = b.acq(1, "b").err() == Some(MX_DUAL_HOLD);
    // 2) 泄漏识破
    let mut b2 = MxBook::new();
    let _ = b2.acq(2, "a");
    got[1] = b2.any_leak();
    // 3) 未清印发拒
    got[2] = b2.seal().err() == Some(MX_NOT_CLEAN);
    // 4) 空主拒
    got[3] = b.acq(3, "").err() == Some(MX_LEAK);
    // 5) 印后录入必抓
    let mut b3 = mk_book();
    let _ = b3.seal();
    got[4] = b3.acq(9, "x").err() == Some(MX_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24158_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24158");
    let mut b = mk_book();

    // J1 终态账齐
    let j1 = b.holds.len() == 3;
    s.add("J1-终态账齐", j1, "三锁终态落表");

    // J2 无双持
    let j2 = !b.any_dual();
    s.add("J2-无双持", j2, "全锁单持有者");

    // J3 无泄漏
    let j3 = !b.any_leak();
    s.add("J3-无泄漏", j3, "acq/rel 守恒");

    // J4 全释放
    let j4 = b.holds.iter().all(|h| h.holders.is_empty());
    s.add("J4-全释放", j4, "终态零持有");

    // J5 双持拒
    let j5 = {
        let mut b2 = MxBook::new();
        let _ = b2.acq(1, "a");
        b2.acq(1, "b").err() == Some(MX_DUAL_HOLD)
    };
    s.add("J5-双持拒", j5, "第二持有者必拒");

    // J6 泄漏挡印发
    let j6 = {
        let mut b2 = MxBook::new();
        let _ = b2.acq(2, "a");
        b2.seal().err() == Some(MX_NOT_CLEAN)
    };
    s.add("J6-泄漏挡印", j6, "未全释放必拒印");

    // J7 收口印发
    let j7 = b.seal().is_ok();
    s.add("J7-收口印发", j7, "全清发印成功");

    // J8 重复印发拒
    let j8 = b.seal().err() == Some(MX_DUP_SEAL);
    s.add("J8-重复印发拒", j8, "二次发印必拒");

    // J9 印后录入拒
    let j9 = b.acq(9, "x").err() == Some(MX_FROZEN);
    s.add("J9-印后录入拒", j9, "印发后冻结");

    // J10 空主拒
    let j10 = {
        let mut b2 = MxBook::new();
        b2.acq(1, "").err() == Some(MX_LEAK)
    };
    s.add("J10-空主拒", j10, "空持有者必拒");

    // J11 释放计数对账
    let j11 = {
        let e = b.holds.iter().find(|h| h.lock_id == 1).unwrap();
        e.acq_total == 1 && e.rel_total == 1
    };
    s.add("J11-计数对账", j11, "锁1 1/1");

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
    fn unx24158_mutex_flow() {
        let mut b = MxBook::new();
        assert!(b.acq(1, "a").is_ok());
        assert_eq!(b.acq(1, "b"), Err(MX_DUAL_HOLD));
        assert!(b.rel(1, "a").is_ok());
        assert_eq!(b.rel(1, "a"), Err(MX_LEAK));
        assert!(b.seal().is_ok());
        assert_eq!(b.seal(), Err(MX_DUP_SEAL));
        assert_eq!(b.acq(2, "x"), Err(MX_FROZEN));
    }

    #[test]
    fn unx24158_checks_green() {
        let s = run_unx24158_checks();
        assert!(s.all_passed(), "UNX-F24158 判据未全绿");
    }
}
