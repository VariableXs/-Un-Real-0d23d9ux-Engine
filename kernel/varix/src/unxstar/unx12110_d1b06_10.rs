//! UNX-F12110 RtlEqualUnicodeString/RtlCompareUnicodeString 比较族（0x1a060→0x19fe0 段）
//!
//! 卡面：比较族语义——先比长后比元、敏感/不敏感双模、三值序
//! （-1/0/1）、前缀短串排前；比较档登记制。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 先比长：长度不等即序定，逐元比较不发生。
//! - J2 逐元定序：等长时首个异元定序（按敏感模式）。
//! - J3 三值口径：小者 -1、等者 0、大者 1。
//! - J4 不敏感折叠：折叠后比较，等值即 0。
//! - J5 空串拒：空单元序列比较必拒。
//! - J6 登记唯一：比较档编号非零唯一。
//! - J7 印账冻结：seal 后登记必拒。
//! - J8 注入五况全抓。
//!
//! 结构：CmpUnit 比较核（三值）+ CmpRec 比较档 + CmpBook 档册 +
//! 折叠指纹（211 进制卷积）+ 注入核（空串/重号/零号/异长序/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19fe0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19fe0;
pub const CM_EMPTY: u32 = SEG_BASE + 0x01; // 空串
pub const CM_DUP_ID: u32 = SEG_BASE + 0x02; // 重号（含零号）
pub const CM_FROZEN: u32 = SEG_BASE + 0x03; // 印账后登记

/// 单元折叠：a-z 升 A-Z。
fn fold_unit(u: u16) -> u16 {
    if u >= 0x61 && u <= 0x7A {
        u - 0x20
    } else {
        u
    }
}

/// 三值序。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Ord3 {
    Less = 0xFFFF_FFFFu32 as u32, // 以 -1 口径（u32 包装）
    Equal = 0,
    Greater = 1,
}

impl Ord3 {
    pub fn to_i32(&self) -> i32 {
        match self {
            Ord3::Less => -1,
            Ord3::Equal => 0,
            Ord3::Greater => 1,
        }
    }
}

/// 比较核：先比长后比元，敏感/不敏感双模。
pub fn cmp_units(a: &[u16], b: &[u16], insensitive: bool) -> Result<Ord3, u32> {
    if a.is_empty() || b.is_empty() {
        return Err(CM_EMPTY);
    }
    if a.len() != b.len() {
        return Ok(if a.len() < b.len() { Ord3::Less } else { Ord3::Greater });
    }
    for i in 0..a.len() {
        let (x, y) = if insensitive {
            (fold_unit(a[i]), fold_unit(b[i]))
        } else {
            (a[i], b[i])
        };
        if x != y {
            return Ok(if x < y { Ord3::Less } else { Ord3::Greater });
        }
    }
    Ok(Ord3::Equal)
}

/// 相等判定便捷。
pub fn equal_units(a: &[u16], b: &[u16], insensitive: bool) -> Result<bool, u32> {
    cmp_units(a, b, insensitive).map(|o| o == Ord3::Equal)
}

/// 比较档。
#[derive(Clone)]
pub struct CmpRec {
    pub id: u32,
    pub units: Vec<u16>,
    pub tag: String,
}

/// 档册。
pub struct CmpBook {
    pub recs: Vec<CmpRec>,
    pub sealed: bool,
}

impl CmpBook {
    pub fn new() -> CmpBook {
        CmpBook { recs: Vec::new(), sealed: false }
    }

    /// 登记档：空串与重号必拒。
    pub fn put(&mut self, id: u32, units: &[u16], tag: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CM_FROZEN);
        }
        if units.is_empty() {
            return Err(CM_EMPTY);
        }
        if id == 0 || self.recs.iter().any(|r| r.id == id) {
            return Err(CM_DUP_ID);
        }
        self.recs.push(CmpRec {
            id,
            units: units.to_vec(),
            tag: tag.to_string(),
        });
        Ok(())
    }

    fn units_of(&self, id: u32) -> Option<&Vec<u16>> {
        self.recs.iter().find(|r| r.id == id).map(|r| &r.units)
    }

    /// 按档号比较。
    pub fn compare(&self, a: u32, b: u32, insensitive: bool) -> Result<Ord3, u32> {
        let ua = match self.units_of(a) {
            Some(u) => u,
            None => return Err(CM_EMPTY),
        };
        let ub = match self.units_of(b) {
            Some(u) => u,
            None => return Err(CM_EMPTY),
        };
        cmp_units(ua, ub, insensitive)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            for u in &r.units {
                h = h.wrapping_mul(211).wrapping_add(*u as u32);
            }
            for by in r.tag.bytes() {
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
        format!("recs={}", self.recs.len())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, id: u32) -> String {
        match self.recs.iter().find(|r| r.id == id) {
            Some(r) => format!("C{}|n{}", r.id, r.units.len()),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CmpBook::new();
    // 1) 空串登记必拒。
    got[0] = b.put(1, &[], "empty").err() == Some(CM_EMPTY);
    // 2) 空串比较必拒。
    got[1] = cmp_units(&[], &[0x41], false).err() == Some(CM_EMPTY);
    // 3) 重号必拒。
    let _ = b.put(2, &[0x41, 0x42], "a");
    got[2] = b.put(2, &[0x43, 0x44], "b").err() == Some(CM_DUP_ID);
    // 4) 零号必拒。
    got[3] = b.put(0, &[0x41], "z").err() == Some(CM_DUP_ID);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(3, &[0x41], "post").err() == Some(CM_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12110_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12110");

    // J1 先比长。
    let j1 = {
        let a = [0x41u16, 0x42];
        let b = [0x41u16, 0x42, 0x43];
        cmp_units(&a, &b, false) == Ok(Ord3::Less)
            && cmp_units(&b, &a, false) == Ok(Ord3::Greater)
    };
    s.add("J1-先比长", j1, "异长即序定");

    // J2 逐元定序（敏感）。
    let j2 = {
        let a = [0x41u16, 0x42];
        let b = [0x41u16, 0x43];
        cmp_units(&a, &b, false) == Ok(Ord3::Less)
            && cmp_units(&b, &a, false) == Ok(Ord3::Greater)
    };
    s.add("J2-逐元定序", j2, "首个异元定序");

    // J3 三值口径。
    let j3 = {
        let a = [0x41u16];
        let b = [0x41u16];
        cmp_units(&a, &b, false) == Ok(Ord3::Equal)
            && Ord3::Less.to_i32() == -1
            && Ord3::Equal.to_i32() == 0
            && Ord3::Greater.to_i32() == 1
    };
    s.add("J3-三值口径", j3, "-1/0/1 全可表");

    // J4 不敏感折叠等值。
    let j4 = {
        let a = [0x61u16, 0x62];
        let b = [0x41u16, 0x42];
        equal_units(&a, &b, true).unwrap_or(false)
            && !equal_units(&a, &b, false).unwrap_or(true)
    };
    s.add("J4-不敏感等值", j4, "折叠后等、敏感不等");

    // J5 敏感模式序可辨。
    let j5 = {
        let a = [0x61u16];
        let b = [0x41u16];
        cmp_units(&a, &b, false) == Ok(Ord3::Greater)
            && cmp_units(&a, &b, true) == Ok(Ord3::Equal)
    };
    s.add("J5-敏感序可辨", j5, "敏感大、不敏感等");

    // J6 空串拒。
    let j6 = cmp_units(&[], &[], false).err() == Some(CM_EMPTY);
    s.add("J6-空串拒", j6, "空序列必拒");

    // J7 档号比较与无档拒。
    let j7 = {
        let mut b = CmpBook::new();
        let _ = b.put(1, &[0x41, 0x42], "ab");
        let _ = b.put(2, &[0x41, 0x43], "ac");
        b.compare(1, 2, false) == Ok(Ord3::Less)
            && b.compare(1, 9, false).is_err()
    };
    s.add("J7-档号比较", j7, "按档比较且缺档拒");

    // J8 编号唯一。
    let j8 = {
        let mut b = CmpBook::new();
        let _ = b.put(3, &[0x41], "a");
        b.put(3, &[0x42], "b").err() == Some(CM_DUP_ID)
    };
    s.add("J8-编号唯一", j8, "重号必拒");

    // J9 回读渲染。
    let j9 = {
        let mut b = CmpBook::new();
        let _ = b.put(5, &[0x41, 0x42], "ab");
        b.line_of(5).contains("C5|n2") && b.line_of(6) == "missing"
    };
    s.add("J9-回读渲染", j9, "档行含元数");

    // J10 印账冻结。
    let j10 = {
        let mut b = CmpBook::new();
        b.seal();
        b.put(1, &[0x41], "p").err() == Some(CM_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后必拒");

    // J11 指纹幂等。
    let j11 = {
        let mut b = CmpBook::new();
        let _ = b.put(1, &[0x41], "a");
        b.fold() == b.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = CmpBook::new();
    let _ = b.put(1, &[0x64, 0x65, 0x76], "dev");
    let _ = b.put(2, &[0x66, 0x69, 0x6c, 0x65], "file");

    s.add("摘要回读", b.summary().contains("recs=2"), "双档在册");
    s.add("序回读", b.compare(1, 2, true) == Ok(Ord3::Less), "dev < file");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12110_cmp_flow() {
        assert_eq!(cmp_units(&[0x61], &[0x62], false), Ok(Ord3::Less));
        assert_eq!(cmp_units(&[0x61], &[0x41], true), Ok(Ord3::Equal));
        assert_eq!(cmp_units(&[0x61, 0x62], &[0x61], false), Ok(Ord3::Greater));
        assert!(cmp_units(&[], &[0x41], false).is_err());
    }

    #[test]
    fn unx12110_checks_green() {
        let s = run_unx12110_checks();
        assert!(s.all_passed(), "UNX-F12110 判据未全绿");
    }

    #[test]
    fn book_compare_flow() {
        let mut b = CmpBook::new();
        let _ = b.put(1, &[0x41], "a");
        assert_eq!(b.put(1, &[0x42], "b"), Err(CM_DUP_ID));
        assert!(b.compare(1, 1, true).is_ok());
    }
}
