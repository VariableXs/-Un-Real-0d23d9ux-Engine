//! UNX-F12094 OBJECT_ATTRIBUTES 非法组合拒止矩阵（0x19de0 段）
//!
//! 卡面：位组合拒止矩阵——（KERNEL_HANDLE×INHERIT）、
//! （FORCE_ACCESS_CHECK×KERNEL_HANDLE）等非法二元组合逐对显性拒；
//! 未知位与空位前置闸。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 空位前置拒：Attributes 全零必拒。
//! - J2 未知位前置拒：族外位非零必拒。
//! - J3 内核×继承拒：KERNEL_HANDLE 与 INHERIT 同置必拒。
//! - J4 强查×内核拒：FORCE_ACCESS_CHECK 与 KERNEL_HANDLE 同置必拒。
//! - J5 合法组合放行：矩阵外组合逐对可查（矩阵行数与位族一致）。
//! - J6 登记唯一：同实例编号重登必拒。
//! - J7 印账冻结：seal 后登记必拒。
//! - J8 注入五况全抓。
//!
//! 结构：ForbiddenPair 拒止矩阵行 + MxSet 组合档 + MxBook 登记册 +
//! 折叠指纹（211 进制卷积）+ 注入核（空位/未知位/内继/强内/印后登）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19de0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19de0;
pub const MX_EMPTY: u32 = SEG_BASE + 0x01; // 空位
pub const MX_UNKNOWN: u32 = SEG_BASE + 0x02; // 未知位
pub const MX_FORBIDDEN: u32 = SEG_BASE + 0x03; // 非法组合
pub const MX_DUP: u32 = SEG_BASE + 0x04; // 重登（含零号）
pub const MX_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// 位常量。
pub const B_INHERIT: u32 = 0x0000_0002;
pub const B_PERMANENT: u32 = 0x0000_0010;
pub const B_OPEN_IF: u32 = 0x0000_0080;
pub const B_KERNEL_HANDLE: u32 = 0x0000_0200;
pub const B_FORCE_CHECK: u32 = 0x0000_0400;
pub const B_KNOWN: u32 =
    B_INHERIT | B_PERMANENT | B_OPEN_IF | B_KERNEL_HANDLE | B_FORCE_CHECK;

/// 拒止矩阵行：非法二元组合。
#[derive(Clone, Copy)]
pub struct ForbiddenPair {
    pub a: u32,
    pub b: u32,
}

/// 拒止矩阵（模型内两行：内继、强内）。
pub const MATRIX: [ForbiddenPair; 2] = [
    ForbiddenPair { a: B_KERNEL_HANDLE, b: B_INHERIT },
    ForbiddenPair { a: B_FORCE_CHECK, b: B_KERNEL_HANDLE },
];

/// 组合档。
#[derive(Clone, Copy)]
pub struct MxSet {
    pub id: u32,
    pub bits: u32,
}

impl MxSet {
    /// 校验核：空位 → 未知位 → 矩阵逐行。
    pub fn validate(&self) -> Result<(), u32> {
        if self.bits == 0 {
            return Err(MX_EMPTY);
        }
        if self.bits & !B_KNOWN != 0 {
            return Err(MX_UNKNOWN);
        }
        for p in MATRIX.iter() {
            if self.bits & p.a != 0 && self.bits & p.b != 0 {
                return Err(MX_FORBIDDEN);
            }
        }
        Ok(())
    }
    pub fn render(&self) -> String {
        format!("M{}|{:x}", self.id, self.bits)
    }
}

/// 组合登记册。
pub struct MxBook {
    pub recs: Vec<MxSet>,
    pub sealed: bool,
}

impl MxBook {
    pub fn new() -> MxBook {
        MxBook { recs: Vec::new(), sealed: false }
    }

    /// 登记：编号唯一 → 校验核。
    pub fn put(&mut self, m: MxSet) -> Result<(), u32> {
        if self.sealed {
            return Err(MX_FROZEN);
        }
        if m.id == 0 || self.recs.iter().any(|r| r.id == m.id) {
            return Err(MX_DUP);
        }
        m.validate()?;
        self.recs.push(m);
        Ok(())
    }

    pub fn by_id(&self, id: u32) -> Option<&MxSet> {
        self.recs.iter().find(|r| r.id == id)
    }

    /// 矩阵行数（与位族闭集一致性可查）。
    pub fn matrix_rows(&self) -> usize {
        MATRIX.len()
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
        format!("recs={} rows={}", self.recs.len(), MATRIX.len())
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
    let mut b = MxBook::new();
    // 1) 空位必拒。
    got[0] = b.put(MxSet { id: 1, bits: 0 }).err() == Some(MX_EMPTY);
    // 2) 未知位必拒。
    got[1] = b.put(MxSet { id: 1, bits: 0x8000_0000 }).err() == Some(MX_UNKNOWN);
    // 3) 内核×继承必拒。
    got[2] = b.put(MxSet { id: 1, bits: B_KERNEL_HANDLE | B_INHERIT })
        .err() == Some(MX_FORBIDDEN);
    // 4) 强查×内核必拒。
    got[3] = b.put(MxSet { id: 1, bits: B_FORCE_CHECK | B_KERNEL_HANDLE })
        .err() == Some(MX_FORBIDDEN);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(MxSet { id: 1, bits: B_PERMANENT }).err() == Some(MX_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12094_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12094");

    // J1 空位前置拒。
    let j1 = {
        let m = MxSet { id: 1, bits: 0 };
        m.validate().err() == Some(MX_EMPTY)
    };
    s.add("J1-空位拒", j1, "全零必拒");

    // J2 未知位前置拒。
    let j2 = {
        let m = MxSet { id: 1, bits: B_INHERIT | 0x0004_0000 };
        m.validate().err() == Some(MX_UNKNOWN)
    };
    s.add("J2-未知位拒", j2, "族外位必拒");

    // J3 内核×继承拒。
    let j3 = {
        let m = MxSet { id: 1, bits: B_KERNEL_HANDLE | B_INHERIT };
        m.validate().err() == Some(MX_FORBIDDEN)
    };
    s.add("J3-内继拒", j3, "KERNEL×INHERIT 必拒");

    // J4 强查×内核拒。
    let j4 = {
        let m = MxSet { id: 1, bits: B_FORCE_CHECK | B_KERNEL_HANDLE };
        m.validate().err() == Some(MX_FORBIDDEN)
    };
    s.add("J4-强内拒", j4, "FORCE×KERNEL 必拒");

    // J5 合法组合放行（矩阵外全组合）。
    let j5 = {
        let combos = [
            B_INHERIT | B_PERMANENT,
            B_OPEN_IF | B_PERMANENT,
            B_FORCE_CHECK | B_INHERIT,
            B_OPEN_IF | B_KERNEL_HANDLE,
        ];
        combos.iter().all(|c| MxSet { id: 1, bits: *c }.validate().is_ok())
    };
    s.add("J5-合法放行", j5, "矩阵外四组合全过");

    // J6 矩阵行数与位族一致。
    let j6 = {
        let b = MxBook::new();
        b.matrix_rows() == 2 && b.summary().contains("rows=2")
    };
    s.add("J6-矩阵行数", j6, "两行拒止");

    // J7 编号唯一。
    let j7 = {
        let mut b = MxBook::new();
        let m = MxSet { id: 3, bits: B_PERMANENT };
        let _ = b.put(m);
        b.put(m).err() == Some(MX_DUP)
            && b.put(MxSet { id: 0, bits: B_PERMANENT }).err() == Some(MX_DUP)
    };
    s.add("J7-编号唯一", j7, "零号重号皆拒");

    // J8 渲染行。
    let j8 = {
        let mut b = MxBook::new();
        let _ = b.put(MxSet { id: 5, bits: B_OPEN_IF });
        b.line_of(5).contains("80") && b.line_of(6) == "missing"
    };
    s.add("J8-渲染行", j8, "位值十六进制回读");

    // J9 印账冻结。
    let j9 = {
        let mut b = MxBook::new();
        b.seal();
        b.put(MxSet { id: 1, bits: B_INHERIT }).err() == Some(MX_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = MxBook::new();
        let _ = b.put(MxSet { id: 1, bits: B_INHERIT });
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = MxBook::new();
    let _ = b.put(MxSet { id: 1, bits: B_INHERIT | B_PERMANENT });
    let _ = b.put(MxSet { id: 2, bits: B_FORCE_CHECK | B_OPEN_IF });

    s.add("摘要回读", b.summary().contains("recs=2"), "双档在册");
    s.add("组合回读", b.by_id(1).map(|r| r.bits) == Some(0x12), "0x12 组合");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12094_matrix_flow() {
        let ok = MxSet { id: 1, bits: B_INHERIT | B_OPEN_IF };
        assert!(ok.validate().is_ok());
        let bad = MxSet { id: 2, bits: B_KERNEL_HANDLE | B_INHERIT };
        assert_eq!(bad.validate(), Err(MX_FORBIDDEN));
    }

    #[test]
    fn unx12094_checks_green() {
        let s = run_unx12094_checks();
        assert!(s.all_passed(), "UNX-F12094 判据未全绿");
    }

    #[test]
    fn book_queries() {
        let mut b = MxBook::new();
        let _ = b.put(MxSet { id: 4, bits: B_PERMANENT });
        assert!(b.by_id(4).is_some());
        assert!(b.by_id(5).is_none());
    }
}
