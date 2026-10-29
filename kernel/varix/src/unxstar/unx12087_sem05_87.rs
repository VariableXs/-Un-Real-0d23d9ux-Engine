//! UNX-F12087 ObjectName 引用语义与缓冲所有权（0x19d00 段）
//!
//! 卡面：OBJECT_ATTRIBUTES→ObjectName——名缓冲为借用引用：调用方持租约，
//! 内核在调用窗口内使用；租约释放后使用（use-after-free）与双重释放
//! 显性拒；租约编号闭集。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 租约唯一：租约编号非零唯一，重号必拒。
//! - J2 释后使用拒：租约释放后的借用必拒。
//! - J3 双重释放拒：同租约二次释放必拒。
//! - J4 无租约拒：借未知租约必拒。
//! - J5 活租约借用放行：未释放租约窗口内使用合法。
//! - J6 租期账：逐租约可回读（长度/释放态）。
//! - J7 印账冻结：seal 后一切操作必拒。
//! - J8 注入五况全抓。
//!
//! 结构：Lease 租约档 + LeaseBook 租约册 + 借用核 + 折叠指纹
//! （211 进制卷积）+ 注入核（零号/无租约/双重释放/释后使用/印后借）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19d00 独占 0x20）。
pub const SEG_BASE: u32 = 0x19d00;
pub const ON_ZERO: u32 = SEG_BASE + 0x01; // 零号租约
pub const ON_NO_LEASE: u32 = SEG_BASE + 0x02; // 无租约
pub const ON_DOUBLE_FREE: u32 = SEG_BASE + 0x03; // 双重释放
pub const ON_USE_AFTER_FREE: u32 = SEG_BASE + 0x04; // 释后使用
pub const ON_DUP: u32 = SEG_BASE + 0x05; // 重号
pub const ON_FROZEN: u32 = SEG_BASE + 0x06; // 印账后操作

/// 租约档。
#[derive(Clone)]
pub struct Lease {
    pub id: u32,
    pub len: u32,
    pub released: bool,
    pub tag: String,
}

/// 租约册。
pub struct LeaseBook {
    pub leases: Vec<Lease>,
    pub sealed: bool,
}

impl LeaseBook {
    pub fn new() -> LeaseBook {
        LeaseBook { leases: Vec::new(), sealed: false }
    }

    fn find(&self, id: u32) -> Option<usize> {
        self.leases.iter().position(|l| l.id == id)
    }

    /// 开租约：零号与重号必拒，长度非零。
    pub fn open(&mut self, id: u32, len: u32, tag: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(ON_FROZEN);
        }
        if id == 0 {
            return Err(ON_ZERO);
        }
        if self.leases.iter().any(|l| l.id == id) {
            return Err(ON_DUP);
        }
        if len == 0 {
            return Err(ON_NO_LEASE); // 零长租约视作无效租借
        }
        self.leases.push(Lease {
            id,
            len,
            released: false,
            tag: tag.to_string(),
        });
        Ok(())
    }

    /// 释放租约：双重释放必拒。
    pub fn release(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(ON_FROZEN);
        }
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(ON_NO_LEASE),
        };
        if self.leases[i].released {
            return Err(ON_DOUBLE_FREE);
        }
        self.leases[i].released = true;
        Ok(())
    }

    /// 借用核：租约须在且未释放。
    pub fn borrow(&self, id: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(ON_FROZEN);
        }
        match self.find(id) {
            None => Err(ON_NO_LEASE),
            Some(i) => {
                if self.leases[i].released {
                    Err(ON_USE_AFTER_FREE)
                } else {
                    Ok(self.leases[i].len)
                }
            }
        }
    }

    /// 活租约数。
    pub fn live_count(&self) -> usize {
        self.leases.iter().filter(|l| !l.released).count()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for l in &self.leases {
            h = h.wrapping_mul(211).wrapping_add(l.id);
            h = h.wrapping_mul(211).wrapping_add(l.len);
            h = h.wrapping_mul(211).wrapping_add(l.released as u32);
            for by in l.tag.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("leases={} live={}", self.leases.len(), self.live_count())
    }

    /// 诊断面：租约渲染。
    pub fn line_of(&self, id: u32) -> String {
        match self.find(id) {
            Some(i) => format!(
                "L{}|{}|r{}",
                self.leases[i].id, self.leases[i].len, self.leases[i].released as u8
            ),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = LeaseBook::new();
    // 1) 零号租约必拒。
    got[0] = b.open(0, 8, "z").err() == Some(ON_ZERO);
    // 2) 无租约借用必拒。
    got[1] = b.borrow(9).err() == Some(ON_NO_LEASE);
    // 3) 双重释放必拒。
    let _ = b.open(1, 8, "a");
    let _ = b.release(1);
    got[2] = b.release(1).err() == Some(ON_DOUBLE_FREE);
    // 4) 释后使用必拒。
    got[3] = b.borrow(1).err() == Some(ON_USE_AFTER_FREE);
    // 5) 印账后借用必抓。
    b.seal();
    got[4] = b.borrow(1).err() == Some(ON_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12087_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12087");

    // J1 零号租约拒。
    let j1 = {
        let mut b = LeaseBook::new();
        b.open(0, 8, "z").err() == Some(ON_ZERO)
    };
    s.add("J1-零号拒", j1, "零号必拒");

    // J2 重号拒。
    let j2 = {
        let mut b = LeaseBook::new();
        let _ = b.open(2, 8, "a");
        b.open(2, 8, "b").err() == Some(ON_DUP)
    };
    s.add("J2-重号拒", j2, "同号二次必拒");

    // J3 零长租约拒。
    let j3 = {
        let mut b = LeaseBook::new();
        b.open(3, 0, "z").err() == Some(ON_NO_LEASE)
    };
    s.add("J3-零长拒", j3, "零长租约必拒");

    // J4 活租约借用放行。
    let j4 = {
        let mut b = LeaseBook::new();
        let _ = b.open(4, 16, "live");
        b.borrow(4) == Ok(16)
    };
    s.add("J4-活借放行", j4, "窗口内使用合法");

    // J5 双重释放拒。
    let j5 = {
        let mut b = LeaseBook::new();
        let _ = b.open(5, 8, "x");
        let _ = b.release(5);
        b.release(5).err() == Some(ON_DOUBLE_FREE)
    };
    s.add("J5-双重释放拒", j5, "二次释放必拒");

    // J6 释后使用拒。
    let j6 = {
        let mut b = LeaseBook::new();
        let _ = b.open(6, 8, "x");
        let _ = b.release(6);
        b.borrow(6).err() == Some(ON_USE_AFTER_FREE)
    };
    s.add("J6-释后使用拒", j6, "UAF 显性");

    // J7 无租约释放拒。
    let j7 = {
        let mut b = LeaseBook::new();
        b.release(99).err() == Some(ON_NO_LEASE)
    };
    s.add("J7-无租释放拒", j7, "释放未知租约必拒");

    // J8 租期账回读。
    let j8 = {
        let mut b = LeaseBook::new();
        let _ = b.open(8, 24, "rd");
        b.line_of(8).contains("24") && b.line_of(9) == "missing"
    };
    s.add("J8-租期回读", j8, "长度可回读");

    // J9 活租约计数。
    let j9 = {
        let mut b = LeaseBook::new();
        let _ = b.open(1, 8, "a");
        let _ = b.open(2, 8, "b");
        let _ = b.release(1);
        b.live_count() == 1 && b.summary().contains("live=1")
    };
    s.add("J9-活计数", j9, "释放后活数递减");

    // J10 印账冻结。
    let j10 = {
        let mut b = LeaseBook::new();
        b.seal();
        b.open(1, 8, "p").err() == Some(ON_FROZEN)
            && b.borrow(1).err() == Some(ON_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后皆拒");

    // J11 指纹幂等。
    let j11 = {
        let mut b = LeaseBook::new();
        let _ = b.open(1, 8, "a");
        b.fold() == b.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = LeaseBook::new();
    let _ = b.open(1, 32, "name-buf");
    let _ = b.open(2, 16, "name-buf-2");

    s.add("摘要回读", b.summary().contains("leases=2"), "双租在册");
    s.add("借用双查", b.borrow(1).is_ok() && b.borrow(2).is_ok(), "双活可借");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12087_lease_flow() {
        let mut b = LeaseBook::new();
        assert!(b.open(1, 8, "a").is_ok());
        assert_eq!(b.open(1, 8, "b"), Err(ON_DUP));
        assert!(b.borrow(1).is_ok());
        assert!(b.release(1).is_ok());
        assert_eq!(b.borrow(1), Err(ON_USE_AFTER_FREE));
    }

    #[test]
    fn unx12087_checks_green() {
        let s = run_unx12087_checks();
        assert!(s.all_passed(), "UNX-F12087 判据未全绿");
    }

    #[test]
    fn double_free_guard() {
        let mut b = LeaseBook::new();
        let _ = b.open(2, 4, "x");
        let _ = b.release(2);
        assert_eq!(b.release(2), Err(ON_DOUBLE_FREE));
    }
}
