//! UNX-F12084 OBJ_KERNEL_HANDLE 位语义（0x19ca0 段）
//!
//! 卡面：OBJ_KERNEL_HANDLE 位——带位句柄落内核表（句柄值高位 0x80000000
//! 置位），用户表拒收带位值，内核表拒收裸值；两表互斥分账。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 位标签：内核句柄值必置高位 0x80000000，裸值入内核表必拒。
//! - J2 用户表拒位：带位值入用户表必拒。
//! - J3 零句柄拒：零号句柄两表皆拒。
//! - J4 表内唯一：同表同值重登必拒。
//! - J5 分账回读：句柄按位判归属，两表各查各的。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：KernHandle 带位句柄类型 + DualTable 双表账 + 位判定 +
//! 折叠指纹（211 进制卷积）+ 注入核（零号/位入用户/裸入内核/重登/印后登）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19ca0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19ca0;
pub const KH_ZERO: u32 = SEG_BASE + 0x01; // 零句柄
pub const KH_BIT_IN_USER: u32 = SEG_BASE + 0x02; // 位入用户表
pub const KH_BIT_MISSING: u32 = SEG_BASE + 0x03; // 裸值入内核表
pub const KH_DUP: u32 = SEG_BASE + 0x04; // 重登
pub const KH_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// 内核句柄高位标签。
pub const KERN_BIT: u32 = 0x8000_0000;

/// 带位句柄。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KernHandle(pub u32);

impl KernHandle {
    /// 位判定。
    pub fn is_kernel(&self) -> bool {
        self.0 & KERN_BIT != 0
    }
    /// 去位裸值。
    pub fn raw(&self) -> u32 {
        self.0 & !KERN_BIT
    }
    pub fn render(&self) -> String {
        format!("H{:08x}", self.0)
    }
}

/// 双表账。
pub struct DualTable {
    pub user: Vec<u32>,
    pub kern: Vec<u32>,
    pub sealed: bool,
}

impl DualTable {
    pub fn new() -> DualTable {
        DualTable { user: Vec::new(), kern: Vec::new(), sealed: false }
    }

    /// 登记：按位分表；零号、错位、重登皆拒。
    pub fn insert(&mut self, h: KernHandle) -> Result<(), u32> {
        if self.sealed {
            return Err(KH_FROZEN);
        }
        if h.0 == 0 {
            return Err(KH_ZERO);
        }
        if h.is_kernel() {
            if self.kern.contains(&h.0) {
                return Err(KH_DUP);
            }
            self.kern.push(h.0);
            Ok(())
        } else {
            if self.user.contains(&h.0) {
                return Err(KH_DUP);
            }
            self.user.push(h.0);
            Ok(())
        }
    }

    /// 强制入用户表：带位必拒。
    pub fn insert_user_strict(&mut self, h: KernHandle) -> Result<(), u32> {
        if self.sealed {
            return Err(KH_FROZEN);
        }
        if h.0 == 0 {
            return Err(KH_ZERO);
        }
        if h.is_kernel() {
            return Err(KH_BIT_IN_USER);
        }
        if self.user.contains(&h.0) {
            return Err(KH_DUP);
        }
        self.user.push(h.0);
        Ok(())
    }

    /// 强制入内核表：裸值必拒。
    pub fn insert_kern_strict(&mut self, h: KernHandle) -> Result<(), u32> {
        if self.sealed {
            return Err(KH_FROZEN);
        }
        if h.0 == 0 {
            return Err(KH_ZERO);
        }
        if !h.is_kernel() {
            return Err(KH_BIT_MISSING);
        }
        if self.kern.contains(&h.0) {
            return Err(KH_DUP);
        }
        self.kern.push(h.0);
        Ok(())
    }

    /// 两表查值。
    pub fn in_user(&self, h: u32) -> bool {
        self.user.contains(&h)
    }
    pub fn in_kern(&self, h: u32) -> bool {
        self.kern.contains(&h)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for u in &self.user {
            h = h.wrapping_mul(211).wrapping_add(*u);
        }
        for k in &self.kern {
            h = h.wrapping_mul(211).wrapping_add(*k);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("u={} k={}", self.user.len(), self.kern.len())
    }

    /// 诊断面：句柄渲染。
    pub fn line_of(&self, h: u32) -> String {
        if self.in_kern(h) {
            format!("K|{:08x}", h)
        } else if self.in_user(h) {
            format!("U|{:08x}", h)
        } else {
            "missing".to_string()
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = DualTable::new();
    // 1) 零句柄必拒。
    got[0] = t.insert(KernHandle(0)).err() == Some(KH_ZERO);
    // 2) 位入用户表必拒。
    got[1] = t.insert_user_strict(KernHandle(KERN_BIT | 4)).err() == Some(KH_BIT_IN_USER);
    // 3) 裸值入内核表必拒。
    got[2] = t.insert_kern_strict(KernHandle(4)).err() == Some(KH_BIT_MISSING);
    // 4) 重登必拒。
    let _ = t.insert(KernHandle(8));
    got[3] = t.insert(KernHandle(8)).err() == Some(KH_DUP);
    // 5) 印账后登记必抓。
    t.seal();
    got[4] = t.insert(KernHandle(0x10)).err() == Some(KH_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12084_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12084");

    // J1 位判定与去位回读。
    let j1 = {
        let k = KernHandle(KERN_BIT | 0x20);
        let u = KernHandle(0x20);
        k.is_kernel() && !u.is_kernel() && k.raw() == 0x20 && k.render().contains("80000020")
    };
    s.add("J1-位判定", j1, "位/裸值双向可判");

    // J2 自动分表。
    let j2 = {
        let mut t = DualTable::new();
        let _ = t.insert(KernHandle(KERN_BIT | 4));
        let _ = t.insert(KernHandle(6));
        t.in_kern(KERN_BIT | 4) && t.in_user(6) && t.summary().contains("u=1 k=1")
    };
    s.add("J2-自动分表", j2, "按位各落各表");

    // J3 位入用户表拒。
    let j3 = {
        let mut t = DualTable::new();
        t.insert_user_strict(KernHandle(KERN_BIT | 8)).err() == Some(KH_BIT_IN_USER)
    };
    s.add("J3-位入用户拒", j3, "用户表带位必拒");

    // J4 裸值入内核表拒。
    let j4 = {
        let mut t = DualTable::new();
        t.insert_kern_strict(KernHandle(0x10)).err() == Some(KH_BIT_MISSING)
    };
    s.add("J4-裸入内核拒", j4, "内核表裸值必拒");

    // J5 零号拒。
    let j5 = {
        let mut t = DualTable::new();
        t.insert(KernHandle(0)).err() == Some(KH_ZERO)
    };
    s.add("J5-零号拒", j5, "零句柄必拒");

    // J6 重登拒。
    let j6 = {
        let mut t = DualTable::new();
        let _ = t.insert(KernHandle(KERN_BIT | 4));
        t.insert(KernHandle(KERN_BIT | 4)).err() == Some(KH_DUP)
    };
    s.add("J6-重登拒", j6, "同值二次必拒");

    // J7 分账行渲染。
    let j7 = {
        let mut t = DualTable::new();
        let _ = t.insert(KernHandle(9));
        t.line_of(9).contains("U|") && t.line_of(10) == "missing"
    };
    s.add("J7-行渲染", j7, "表属可回读");

    // J8 印账冻结。
    let j8 = {
        let mut t = DualTable::new();
        t.seal();
        t.insert(KernHandle(1)).err() == Some(KH_FROZEN)
            && t.insert_kern_strict(KernHandle(KERN_BIT | 1)).err() == Some(KH_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后皆拒");

    // J9 指纹幂等。
    let j9 = {
        let mut t = DualTable::new();
        let _ = t.insert(KernHandle(4));
        t.fold() == t.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 注入核 5/5。
    let got = inject_five();
    let j10 = got.iter().all(|x| *x);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut t = DualTable::new();
    let _ = t.insert(KernHandle(KERN_BIT | 0x40));
    let _ = t.insert(KernHandle(0x44));
    let _ = t.insert(KernHandle(0x48));

    s.add("摘要回读", t.summary().contains("u=2 k=1"), "双用户一内核");
    s.add("缺席行", t.line_of(0x100) == "missing", "缺值显性");
    s.add("内核行", t.line_of(KERN_BIT | 0x40).contains("K|"), "内核表行渲染");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12084_dual_flow() {
        let mut t = DualTable::new();
        assert!(t.insert(KernHandle(KERN_BIT | 4)).is_ok());
        assert!(t.insert(KernHandle(6)).is_ok());
        assert_eq!(t.insert_user_strict(KernHandle(KERN_BIT | 8)), Err(KH_BIT_IN_USER));
        assert_eq!(t.insert_kern_strict(KernHandle(8)), Err(KH_BIT_MISSING));
    }

    #[test]
    fn unx12084_checks_green() {
        let s = run_unx12084_checks();
        assert!(s.all_passed(), "UNX-F12084 判据未全绿");
    }

    #[test]
    fn handle_bit_helpers() {
        let h = KernHandle(KERN_BIT | 0x11);
        assert!(h.is_kernel());
        assert_eq!(h.raw(), 0x11);
        assert!(!KernHandle(0x11).is_kernel());
    }
}
