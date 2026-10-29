//! UNX-F12083 OBJ_INHERIT 位语义（0x19c80 段）
//!
//! 卡面：OBJ_INHERIT 位——进程创建时句柄表继承：仅带 INHERIT 位的句柄
//! 进入子进程表；孤行/重登/复继承显性拒。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 标记唯一：父表句柄编号非零唯一，重号必拒。
//! - J2 继承筛选：仅 INHERIT 位句柄被复制进子表。
//! - J3 孤行拒：子行父编号在父表无源必拒。
//! - J4 复继承拒：同父句柄二次复制必拒。
//! - J5 非继承不可见：无 INHERIT 位句柄不出现在子表。
//! - J6 印账冻结：seal 后继承必拒。
//! - J7 注入五况全抓。
//!
//! 结构：InheritRow 句柄行 + InheritBook 双表（父表 + 子表）+ 继承核 +
//! 折叠指纹（211 进制卷积）+ 注入核（零号/重标/孤行/复继承/印后继承）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19c80 独占 0x20）。
pub const SEG_BASE: u32 = 0x19c80;
pub const IN_ZERO: u32 = SEG_BASE + 0x01; // 零号句柄
pub const IN_DUP: u32 = SEG_BASE + 0x02; // 重标
pub const IN_ORPHAN: u32 = SEG_BASE + 0x03; // 孤行
pub const IN_RECOPY: u32 = SEG_BASE + 0x04; // 复继承
pub const IN_FROZEN: u32 = SEG_BASE + 0x05; // 印账后继承

/// 父表句柄行。
#[derive(Clone, Copy)]
pub struct InheritRow {
    pub handle: u32,
    pub inherit: bool,
}

/// 双表继承账。
pub struct InheritBook {
    pub parent: Vec<InheritRow>,
    pub child: Vec<u32>,
    pub inherited_once: bool,
    pub sealed: bool,
}

impl InheritBook {
    pub fn new() -> InheritBook {
        InheritBook {
            parent: Vec::new(),
            child: Vec::new(),
            inherited_once: false,
            sealed: false,
        }
    }

    /// 父表标记：零号与重号必拒。
    pub fn mark(&mut self, handle: u32, inherit: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(IN_FROZEN);
        }
        if handle == 0 {
            return Err(IN_ZERO);
        }
        if self.parent.iter().any(|r| r.handle == handle) {
            return Err(IN_DUP);
        }
        self.parent.push(InheritRow { handle, inherit });
        Ok(())
    }

    /// 继承核：复制 INHERIT 位行至子表；孤行与复继承必拒。
    pub fn run_inherit(&mut self, child_rows: &[u32]) -> Result<usize, u32> {
        if self.sealed {
            return Err(IN_FROZEN);
        }
        if self.inherited_once {
            return Err(IN_RECOPY);
        }
        for h in child_rows {
            if !self.parent.iter().any(|r| r.handle == *h) {
                return Err(IN_ORPHAN);
            }
        }
        let mut copied = 0usize;
        for r in &self.parent {
            if r.inherit {
                if !self.child.contains(&r.handle) {
                    self.child.push(r.handle);
                    copied += 1;
                }
            }
        }
        self.inherited_once = true;
        Ok(copied)
    }

    /// 子表可见判定。
    pub fn child_has(&self, handle: u32) -> bool {
        self.child.contains(&handle)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.parent {
            h = h.wrapping_mul(211).wrapping_add(r.handle);
            h = h.wrapping_mul(211).wrapping_add(r.inherit as u32);
        }
        for c in &self.child {
            h = h.wrapping_mul(211).wrapping_add(*c);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("p={} c={}", self.parent.len(), self.child.len())
    }

    /// 诊断面：父行渲染。
    pub fn row_line(&self, handle: u32) -> String {
        match self.parent.iter().find(|r| r.handle == handle) {
            Some(r) => format!("H{}|i{}", handle, r.inherit as u8),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = InheritBook::new();
    // 1) 零号句柄必拒。
    got[0] = b.mark(0, true).err() == Some(IN_ZERO);
    // 2) 重标必拒。
    let _ = b.mark(4, true);
    got[1] = b.mark(4, false).err() == Some(IN_DUP);
    // 3) 孤行必拒：子行父编号无源。
    let _ = b.mark(5, true);
    got[2] = b.run_inherit(&[99]).err() == Some(IN_ORPHAN);
    // 4) 复继承必拒（孤行已使首次未落账，先成账再复跑）。
    let _ = b.run_inherit(&[]);
    got[3] = b.run_inherit(&[5]).err() == Some(IN_RECOPY);
    // 5) 印账后继承必抓。
    b.seal();
    got[4] = b.run_inherit(&[]).err() == Some(IN_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12083_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12083");

    // J1 零号拒。
    let j1 = {
        let mut b = InheritBook::new();
        b.mark(0, true).err() == Some(IN_ZERO)
    };
    s.add("J1-零号拒", j1, "零号句柄必拒");

    // J2 重标拒。
    let j2 = {
        let mut b = InheritBook::new();
        let _ = b.mark(2, true);
        b.mark(2, true).err() == Some(IN_DUP)
    };
    s.add("J2-重标拒", j2, "同句柄二次必拒");

    // J3 继承筛选：仅 INHERIT 位行进子表。
    let j3 = {
        let mut b = InheritBook::new();
        let _ = b.mark(2, true);
        let _ = b.mark(3, false);
        let copied = b.run_inherit(&[]).unwrap_or(0);
        copied == 1 && b.child_has(2) && !b.child_has(3)
    };
    s.add("J3-继承筛选", j3, "无位句柄不可见");

    // J4 孤行拒。
    let j4 = {
        let mut b = InheritBook::new();
        b.run_inherit(&[7]).err() == Some(IN_ORPHAN)
    };
    s.add("J4-孤行拒", j4, "无源父编号必拒");

    // J5 复继承拒。
    let j5 = {
        let mut b = InheritBook::new();
        let _ = b.mark(2, true);
        let _ = b.run_inherit(&[]);
        b.run_inherit(&[]).err() == Some(IN_RECOPY)
    };
    s.add("J5-复继承拒", j5, "二次继承必拒");

    // J6 非继承句柄不触发孤儿误判：子行只报无源者。
    let j6 = {
        let mut b = InheritBook::new();
        let _ = b.mark(2, true);
        let _ = b.mark(4, false);
        b.run_inherit(&[]).is_ok() && !b.child_has(4)
    };
    s.add("J6-非继承不可见", j6, "无位行静默过滤");

    // J7 行渲染。
    let j7 = {
        let mut b = InheritBook::new();
        let _ = b.mark(8, true);
        b.row_line(8).contains("i1") && b.row_line(9) == "missing"
    };
    s.add("J7-行渲染", j7, "INHERIT 位可回读");

    // J8 印账冻结。
    let j8 = {
        let mut b = InheritBook::new();
        b.seal();
        b.mark(2, true).err() == Some(IN_FROZEN)
            && b.run_inherit(&[]).err() == Some(IN_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后皆拒");

    // J9 指纹幂等。
    let j9 = {
        let mut b = InheritBook::new();
        let _ = b.mark(2, true);
        b.fold() == b.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 注入核 5/5。
    let got = inject_five();
    let j10 = got.iter().all(|x| *x);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = InheritBook::new();
    let _ = b.mark(2, true);
    let _ = b.mark(3, true);
    let _ = b.mark(4, false);
    let _ = b.run_inherit(&[]);

    s.add("摘要回读", b.summary().contains("p=3 c=2"), "三父两子");
    s.add("多继承计数", b.child.len() == 2, "双 INHERIT 行进子表");
    s.add("缺席行", b.row_line(99) == "missing", "缺行显性");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12083_inherit_flow() {
        let mut b = InheritBook::new();
        assert!(b.mark(2, true).is_ok());
        assert!(b.mark(3, false).is_ok());
        assert_eq!(b.run_inherit(&[]), Ok(1));
        assert!(b.child_has(2));
        assert!(!b.child_has(3));
        assert_eq!(b.run_inherit(&[]), Err(IN_RECOPY));
    }

    #[test]
    fn unx12083_checks_green() {
        let s = run_unx12083_checks();
        assert!(s.all_passed(), "UNX-F12083 判据未全绿");
    }

    #[test]
    fn orphan_guard() {
        let mut b = InheritBook::new();
        assert_eq!(b.run_inherit(&[42]), Err(IN_ORPHAN));
    }
}
