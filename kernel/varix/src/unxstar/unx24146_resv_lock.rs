//! UNX-F24146 reservation 锁与显存互斥纪律·子功能06（持锁者表与互斥校验）（0x41420 段）
//!
//! 锚点：UNX-F24146-J1 持有者账（锁→持有者映射逐条落账）；J2 互斥语
//! 义（一锁一主 + 一主多锁 + 校验一致性三性质）；J3 互斥指纹（账折
//! 叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（持锁者互斥校验面）。
//!
//! 结构：锁→主映射表 + 迁移核 + 互斥校验核 + 折叠指纹 + 注入核（双
//! 主/野锁/自证伪/印后/重号必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41420 独占 0x20）。
pub const SEG_BASE: u32 = 0x41420;
pub const MH_DUAL_OWNER: u32 = SEG_BASE + 0x01; // 双主
pub const MH_UNK_LOCK: u32 = SEG_BASE + 0x02; // 野锁
pub const MH_SELF_CHECK: u32 = SEG_BASE + 0x03; // 自证伪档
pub const MH_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const MH_EMPTY: u32 = SEG_BASE + 0x05; // 空主

/// 锁→主映射。
#[derive(Clone, PartialEq)]
pub struct OwnMap {
    pub lock_id: u32,
    pub owner: String,
}

/// 互斥簿。
pub struct MhBook {
    pub maps: Vec<OwnMap>,
    pub sealed: bool,
}

impl MhBook {
    pub fn new() -> MhBook {
        MhBook { maps: Vec::new(), sealed: false }
    }

    /// 登记持锁：一锁一主，重号即拒（互斥核心）。
    pub fn hold(&mut self, lock_id: u32, owner: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MH_FROZEN);
        }
        if owner.is_empty() {
            return Err(MH_EMPTY);
        }
        if self.maps.iter().any(|m| m.lock_id == lock_id) {
            return Err(MH_DUAL_OWNER);
        }
        self.maps.push(OwnMap { lock_id, owner: owner.to_string() });
        Ok(())
    }

    /// 迁移：原主放行新主接手。
    pub fn transfer(&mut self, lock_id: u32, from: &str, to: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MH_FROZEN);
        }
        if to.is_empty() || from == to {
            return Err(MH_SELF_CHECK);
        }
        let m = match self.maps.iter_mut().find(|m| m.lock_id == lock_id) {
            Some(m) => m,
            None => return Err(MH_UNK_LOCK),
        };
        if m.owner != from {
            return Err(MH_DUAL_OWNER);
        }
        m.owner = to.to_string();
        Ok(())
    }

    /// 释放：仅原主。
    pub fn release(&mut self, lock_id: u32, owner: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MH_FROZEN);
        }
        let idx = match self.maps.iter().position(|m| m.lock_id == lock_id) {
            Some(i) => i,
            None => return Err(MH_UNK_LOCK),
        };
        if self.maps[idx].owner != owner {
            return Err(MH_DUAL_OWNER);
        }
        self.maps.remove(idx);
        Ok(())
    }

    pub fn owner_of(&self, lock_id: u32) -> Option<&str> {
        self.maps.iter().find(|m| m.lock_id == lock_id).map(|m| m.owner.as_str())
    }

    /// 互斥校验：无重号锁。
    pub fn mutex_ok(&self) -> bool {
        for i in 0..self.maps.len() {
            for j in (i + 1)..self.maps.len() {
                if self.maps[i].lock_id == self.maps[j].lock_id {
                    return false;
                }
            }
        }
        true
    }

    pub fn locks_of(&self, owner: &str) -> Vec<u32> {
        self.maps.iter().filter(|m| m.owner == owner).map(|m| m.lock_id).collect()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41420;
        for m in &self.maps {
            h = h.wrapping_mul(31).wrapping_add(m.lock_id);
            for by in m.owner.bytes() {
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

/// 标准样本：四锁三主。
pub fn mk_book() -> MhBook {
    let mut b = MhBook::new();
    let _ = b.hold(1, "flip");
    let _ = b.hold(2, "dma");
    let _ = b.hold(3, "dma");
    let _ = b.hold(4, "cmd");
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 双主
    got[0] = b.hold(1, "other").err() == Some(MH_DUAL_OWNER);
    // 2) 野锁迁移
    got[1] = b.transfer(99, "x", "y").err() == Some(MH_UNK_LOCK);
    // 3) 自证伪迁移（from==to）
    got[2] = b.transfer(1, "flip", "flip").err() == Some(MH_SELF_CHECK);
    // 4) 非原主释放
    got[3] = b.release(1, "other").err() == Some(MH_DUAL_OWNER);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.hold(9, "x").err() == Some(MH_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24146_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24146");
    let mut b = mk_book();

    // J1 映射账齐
    let j1 = b.maps.len() == 4;
    s.add("J1-映射账齐", j1, "四锁映射全落");

    // J2 一锁一主
    let j2 = b.mutex_ok();
    s.add("J2-一锁一主", j2, "无重号锁映射");

    // J3 一主多锁
    let j3 = b.locks_of("dma").len() == 2;
    s.add("J3-一主多锁", j3, "dma 持锁 2/3");

    // J4 迁移换主
    let j4 = {
        let mut b2 = mk_book();
        b2.transfer(1, "flip", "cmd").is_ok()
            && b2.owner_of(1) == Some("cmd")
    };
    s.add("J4-迁移换主", j4, "原主放行新主接手");

    // J5 双主拒
    let j5 = b.hold(1, "other").err() == Some(MH_DUAL_OWNER);
    s.add("J5-双主拒", j5, "已持锁再登记必拒");

    // J6 野锁拒
    let j6 = b.transfer(99, "x", "y").err() == Some(MH_UNK_LOCK);
    s.add("J6-野锁拒", j6, "未登记锁必拒");

    // J7 自迁移拒
    let j7 = b.transfer(1, "flip", "flip").err() == Some(MH_SELF_CHECK);
    s.add("J7-自迁移拒", j7, "from==to 必拒");

    // J8 非原主迁移拒
    let j8 = b.transfer(1, "other", "cmd").err() == Some(MH_DUAL_OWNER);
    s.add("J8-非原主迁移拒", j8, "假原主必拒");

    // J9 释放清映射
    let j9 = {
        let mut b2 = mk_book();
        b2.release(4, "cmd").is_ok() && b2.owner_of(4).is_none()
    };
    s.add("J9-释放清映射", j9, "释放后映射移除");

    // J10 空主拒
    let j10 = b.hold(9, "").err() == Some(MH_EMPTY);
    s.add("J10-空主拒", j10, "空持有者必拒");

    // J11 释放后可重登
    let j11 = {
        let mut b2 = mk_book();
        let _ = b2.release(4, "cmd");
        b2.hold(4, "flip").is_ok()
    };
    s.add("J11-释放重登", j11, "清映射后他人可持");

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
    fn unx24146_mutex_flow() {
        let mut b = mk_book();
        assert_eq!(b.hold(1, "x"), Err(MH_DUAL_OWNER));
        assert!(b.transfer(1, "flip", "cmd").is_ok());
        assert_eq!(b.transfer(1, "flip", "y"), Err(MH_DUAL_OWNER));
        assert!(b.release(1, "cmd").is_ok());
        assert!(b.hold(1, "flip").is_ok());
        b.seal();
        assert_eq!(b.hold(9, "x"), Err(MH_FROZEN));
    }

    #[test]
    fn unx24146_checks_green() {
        let s = run_unx24146_checks();
        assert!(s.all_passed(), "UNX-F24146 判据未全绿");
    }
}
