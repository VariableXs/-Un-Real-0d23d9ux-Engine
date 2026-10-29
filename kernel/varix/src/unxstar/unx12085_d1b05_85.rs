//! UNX-F12085 OBJ_FORCE_ACCESS_CHECK/OPEN_IF/PERMANENT 位族（0x19cc0 段）
//!
//! 卡面：OBJ_FORCE_ACCESS_CHECK（0x400）/ OBJ_OPEN_IF（0x80）/
//! OBJ_PERMANENT（0x10）位族语义——强制访问检查与内核句柄互斥、
//! PERMANENT 推迟删除、OPEN_IF 仅建态有意义；位族登记册逐条过闸。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 强查互斥：FORCE_ACCESS_CHECK 与 OBJ_KERNEL_HANDLE（0x200）同置必拒。
//! - J2 保留位零：超出已知位族的位非零必拒。
//! - J3 空位拒：Attributes 全零视作未初始化必拒。
//! - J4 PERMANENT 语义：置位后删除请求被折留（deletion deferred 可查）。
//! - J5 OPEN_IF 建态挂钩：OPEN_IF 置位时 create-if-exists 语义可查。
//! - J6 登记唯一：同实例编号重登必拒。
//! - J7 印账冻结：seal 后登记必拒。
//! - J8 注入五况全抓。
//!
//! 结构：FlagSet 位族校验 + FlagBook 登记册 + PERMANENT/OPEN_IF 语义面 +
//! 折叠指纹（211 进制卷积）+ 注入核（空位/保留位/互斥/重登/印后登）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19cc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19cc0;
pub const GB_EMPTY: u32 = SEG_BASE + 0x01; // 空位
pub const GB_UNKNOWN: u32 = SEG_BASE + 0x02; // 保留位非零
pub const GB_CONFLICT: u32 = SEG_BASE + 0x03; // 强查互斥
pub const GB_DUP: u32 = SEG_BASE + 0x04; // 重登（含零号）
pub const GB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// 位族常量。
pub const F_PERMANENT: u32 = 0x0000_0010;
pub const F_OPEN_IF: u32 = 0x0000_0080;
pub const F_KERNEL_HANDLE: u32 = 0x0000_0200;
pub const F_FORCE_CHECK: u32 = 0x0000_0400;
/// 已知位族掩码。
pub const KNOWN_MASK: u32 =
    F_PERMANENT | F_OPEN_IF | F_KERNEL_HANDLE | F_FORCE_CHECK;

/// 位族集合。
#[derive(Clone, Copy)]
pub struct FlagSet {
    pub id: u32,
    pub bits: u32,
}

impl FlagSet {
    /// 位族校验：空位 → 保留位 → 互斥。
    pub fn validate(&self) -> Result<(), u32> {
        if self.bits == 0 {
            return Err(GB_EMPTY);
        }
        if self.bits & !KNOWN_MASK != 0 {
            return Err(GB_UNKNOWN);
        }
        if self.bits & F_FORCE_CHECK != 0 && self.bits & F_KERNEL_HANDLE != 0 {
            return Err(GB_CONFLICT);
        }
        Ok(())
    }
    pub fn permanent(&self) -> bool {
        self.bits & F_PERMANENT != 0
    }
    pub fn open_if(&self) -> bool {
        self.bits & F_OPEN_IF != 0
    }
    pub fn render(&self) -> String {
        format!("F{}|{:x}", self.id, self.bits)
    }
}

/// 位族登记册。
pub struct FlagBook {
    pub recs: Vec<FlagSet>,
    pub sealed: bool,
}

impl FlagBook {
    pub fn new() -> FlagBook {
        FlagBook { recs: Vec::new(), sealed: false }
    }

    /// 登记：编号唯一 → 位族校验。
    pub fn put(&mut self, f: FlagSet) -> Result<(), u32> {
        if self.sealed {
            return Err(GB_FROZEN);
        }
        if f.id == 0 || self.recs.iter().any(|r| r.id == f.id) {
            return Err(GB_DUP);
        }
        f.validate()?;
        self.recs.push(f);
        Ok(())
    }

    /// 回读。
    pub fn by_id(&self, id: u32) -> Option<&FlagSet> {
        self.recs.iter().find(|r| r.id == id)
    }

    /// PERMANENT 删除折留判定。
    pub fn deletion_deferred(&self, id: u32) -> Option<bool> {
        self.by_id(id).map(|r| r.permanent())
    }

    /// OPEN_IF 建态挂钩判定。
    pub fn create_if_exists(&self, id: u32) -> Option<bool> {
        self.by_id(id).map(|r| r.open_if())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            h = h.wrapping_mul(211).wrapping_add(r.bits);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("recs={}", self.recs.len())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, id: u32) -> String {
        match self.by_id(id) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = FlagBook::new();
    // 1) 空位必拒。
    got[0] = b.put(FlagSet { id: 1, bits: 0 }).err() == Some(GB_EMPTY);
    // 2) 保留位非零必拒。
    got[1] = b.put(FlagSet { id: 1, bits: 0x0001_0000 }).err() == Some(GB_UNKNOWN);
    // 3) 强查互斥必拒。
    got[2] = b.put(FlagSet { id: 1, bits: F_FORCE_CHECK | F_KERNEL_HANDLE })
        .err() == Some(GB_CONFLICT);
    // 4) 重登必拒。
    let _ = b.put(FlagSet { id: 2, bits: F_PERMANENT });
    got[3] = b.put(FlagSet { id: 2, bits: F_OPEN_IF }).err() == Some(GB_DUP);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(FlagSet { id: 3, bits: F_PERMANENT }).err() == Some(GB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12085_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12085");

    // J1 强查互斥。
    let j1 = {
        let f = FlagSet { id: 1, bits: F_FORCE_CHECK | F_KERNEL_HANDLE };
        f.validate().err() == Some(GB_CONFLICT)
    };
    s.add("J1-强查互斥", j1, "同置必拒");

    // J2 保留位零。
    let j2 = {
        let f = FlagSet { id: 1, bits: F_PERMANENT | 0x4000_0000 };
        f.validate().err() == Some(GB_UNKNOWN)
    };
    s.add("J2-保留位零", j2, "族外位必拒");

    // J3 空位拒。
    let j3 = {
        let f = FlagSet { id: 1, bits: 0 };
        f.validate().err() == Some(GB_EMPTY)
    };
    s.add("J3-空位拒", j3, "未初始化必拒");

    // J4 PERMANENT 删除折留。
    let j4 = {
        let mut b = FlagBook::new();
        let _ = b.put(FlagSet { id: 5, bits: F_PERMANENT | F_OPEN_IF });
        b.deletion_deferred(5) == Some(true)
            && b.create_if_exists(5) == Some(true)
    };
    s.add("J4-永久折留", j4, "PERMANENT/OPEN_IF 双面可查");

    // J5 非永久即时删除。
    let j5 = {
        let mut b = FlagBook::new();
        let _ = b.put(FlagSet { id: 6, bits: F_KERNEL_HANDLE });
        b.deletion_deferred(6) == Some(false)
    };
    s.add("J5-非永久即时", j5, "无位句柄删除即时");

    // J6 单位族合法。
    let j6 = {
        let f1 = FlagSet { id: 1, bits: F_FORCE_CHECK };
        let f2 = FlagSet { id: 2, bits: F_KERNEL_HANDLE };
        f1.validate().is_ok() && f2.validate().is_ok()
    };
    s.add("J6-单位合法", j6, "任一单置皆过");

    // J7 零号与重号拒。
    let j7 = {
        let mut b = FlagBook::new();
        let f = FlagSet { id: 8, bits: F_OPEN_IF };
        let _ = b.put(f);
        b.put(f).err() == Some(GB_DUP)
            && b.put(FlagSet { id: 0, bits: F_OPEN_IF }).err() == Some(GB_DUP)
    };
    s.add("J7-编号唯一", j7, "零号重号皆拒");

    // J8 回读与渲染。
    let j8 = {
        let mut b = FlagBook::new();
        let _ = b.put(FlagSet { id: 3, bits: F_PERMANENT });
        b.line_of(3).contains("10") && b.line_of(4) == "missing"
    };
    s.add("J8-回读渲染", j8, "行含位值");

    // J9 印账冻结。
    let j9 = {
        let mut b = FlagBook::new();
        b.seal();
        b.put(FlagSet { id: 1, bits: F_OPEN_IF }).err() == Some(GB_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = FlagBook::new();
        let _ = b.put(FlagSet { id: 1, bits: F_OPEN_IF });
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = FlagBook::new();
    let _ = b.put(FlagSet { id: 1, bits: F_PERMANENT });
    let _ = b.put(FlagSet { id: 2, bits: F_FORCE_CHECK });

    s.add("摘要回读", b.summary().contains("recs=2"), "双档在册");
    s.add("折留查询", b.deletion_deferred(1) == Some(true), "1 号折留");
    s.add("缺席行", b.deletion_deferred(9).is_none(), "缺档显性");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12085_flag_flow() {
        let f = FlagSet { id: 1, bits: F_OPEN_IF | F_PERMANENT };
        assert!(f.validate().is_ok());
        assert!(f.permanent() && f.open_if());
        let bad = FlagSet { id: 2, bits: F_FORCE_CHECK | F_KERNEL_HANDLE };
        assert_eq!(bad.validate(), Err(GB_CONFLICT));
    }

    #[test]
    fn unx12085_checks_green() {
        let s = run_unx12085_checks();
        assert!(s.all_passed(), "UNX-F12085 判据未全绿");
    }

    #[test]
    fn book_queries() {
        let mut b = FlagBook::new();
        let _ = b.put(FlagSet { id: 4, bits: F_OPEN_IF });
        assert_eq!(b.create_if_exists(4), Some(true));
        assert!(b.by_id(9).is_none());
    }
}
