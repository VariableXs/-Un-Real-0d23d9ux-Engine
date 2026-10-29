//! UNX-F12111 RtlPrefixUnicodeString 前缀判定（0x1a000 段）
//!
//! 卡面：RtlPrefixUnicodeString——前缀判定：前缀长 ≤ 串长、逐元折叠
//! 可选、空前缀拒、越界前缀拒。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 空前缀拒：空单元序列前缀必拒。
//! - J2 越界前缀拒：前缀长于串必拒（非前缀语义而按越界显性分码）。
//! - J3 敏感前缀：逐元精确。
//! - J4 不敏感前缀：折叠后逐元。
//! - J5 真前缀正路径：完整前缀命中。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：prefix_check 前缀核 + PrefRec 判定档 + PrefBook 档册 +
//! 折叠指纹（211 进制卷积）+ 注入核（空前缀/越界前缀/重号/零号/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a000 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a000;
pub const PF_EMPTY: u32 = SEG_BASE + 0x01; // 空前缀
pub const PF_LONGER: u32 = SEG_BASE + 0x02; // 越界前缀
pub const PF_DUP_ID: u32 = SEG_BASE + 0x03; // 重号（含零号）
pub const PF_FROZEN: u32 = SEG_BASE + 0x04; // 印账后登记

/// 单元折叠。
fn fold_unit(u: u16) -> u16 {
    if u >= 0x61 && u <= 0x7A {
        u - 0x20
    } else {
        u
    }
}

/// 前缀核：敏感/不敏感双模。
pub fn prefix_check(prefix: &[u16], s: &[u16], insensitive: bool) -> Result<bool, u32> {
    if prefix.is_empty() {
        return Err(PF_EMPTY);
    }
    if prefix.len() > s.len() {
        return Err(PF_LONGER);
    }
    for i in 0..prefix.len() {
        let (x, y) = if insensitive {
            (fold_unit(prefix[i]), fold_unit(s[i]))
        } else {
            (prefix[i], s[i])
        };
        if x != y {
            return Ok(false);
        }
    }
    Ok(true)
}

/// 判定档。
#[derive(Clone)]
pub struct PrefRec {
    pub id: u32,
    pub prefix: Vec<u16>,
    pub subject: Vec<u16>,
    pub tag: String,
}

/// 档册。
pub struct PrefBook {
    pub recs: Vec<PrefRec>,
    pub sealed: bool,
}

impl PrefBook {
    pub fn new() -> PrefBook {
        PrefBook { recs: Vec::new(), sealed: false }
    }

    /// 登记判定档：空前缀与重号必拒。
    pub fn put(&mut self, id: u32, prefix: &[u16], subject: &[u16], tag: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PF_FROZEN);
        }
        if prefix.is_empty() {
            return Err(PF_EMPTY);
        }
        if id == 0 || self.recs.iter().any(|r| r.id == id) {
            return Err(PF_DUP_ID);
        }
        self.recs.push(PrefRec {
            id,
            prefix: prefix.to_vec(),
            subject: subject.to_vec(),
            tag: tag.to_string(),
        });
        Ok(())
    }

    /// 按档判定：不敏感模式。
    pub fn judge(&self, id: u32, insensitive: bool) -> Result<bool, u32> {
        match self.recs.iter().find(|r| r.id == id) {
            None => Err(PF_EMPTY),
            Some(r) => prefix_check(&r.prefix, &r.subject, insensitive),
        }
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            for u in &r.prefix {
                h = h.wrapping_mul(211).wrapping_add(*u as u32);
            }
            for u in &r.subject {
                h = h.wrapping_mul(211).wrapping_add(*u as u32);
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
            Some(r) => format!("F{}|p{}|s{}", r.id, r.prefix.len(), r.subject.len()),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = PrefBook::new();
    // 1) 空前缀登记必拒。
    got[0] = b.put(1, &[], &[0x41], "e").err() == Some(PF_EMPTY);
    // 2) 越界前缀必拒。
    got[1] = prefix_check(&[0x41, 0x42, 0x43], &[0x41, 0x42], false)
        .err() == Some(PF_LONGER);
    // 3) 重号必拒。
    let _ = b.put(2, &[0x41], &[0x41, 0x42], "a");
    got[2] = b.put(2, &[0x43], &[0x43, 0x44], "b").err() == Some(PF_DUP_ID);
    // 4) 零号必拒。
    got[3] = b.put(0, &[0x41], &[0x41], "z").err() == Some(PF_DUP_ID);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(3, &[0x41], &[0x41], "post").err() == Some(PF_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12111_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12111");
    const DEV: [u16; 8] = [0x64, 0x65, 0x76, 0x69, 0x63, 0x65, 0x73, 0x5C];

    // J1 空前缀拒。
    let j1 = prefix_check(&[], &DEV, false).err() == Some(PF_EMPTY);
    s.add("J1-空前缀拒", j1, "空序列必拒");

    // J2 越界前缀拒。
    let j2 = prefix_check(&DEV, &DEV[..4], false).err() == Some(PF_LONGER);
    s.add("J2-越界前缀拒", j2, "前缀长于串必拒");

    // J3 敏感前缀正命中。
    let j3 = prefix_check(&DEV[..3], &DEV, false) == Ok(true);
    s.add("J3-敏感命中", j3, "dev 前缀命中");

    // J4 敏感失配。
    let j4 = {
        let pfx = [0x44u16, 0x45, 0x56]; // "DEV" 大写
        prefix_check(&pfx, &DEV, false) == Ok(false)
    };
    s.add("J4-敏感失配", j4, "大小写异即否");

    // J5 不敏感前缀命中。
    let j5 = {
        let pfx = [0x44u16, 0x45, 0x56];
        prefix_check(&pfx, &DEV, true) == Ok(true)
    };
    s.add("J5-不敏感命中", j5, "折叠后命中");

    // J6 中段失配。
    let j6 = {
        let pfx = [0x64u16, 0x78]; // 第二元不匹配
        prefix_check(&pfx, &DEV, false) == Ok(false)
    };
    s.add("J6-中段失配", j6, "首元同中段异即否");

    // J7 恰等长前缀。
    let j7 = prefix_check(&DEV, &DEV, false) == Ok(true);
    s.add("J7-恰等长", j7, "全等即前缀");

    // J8 档判定与无档拒。
    let j8 = {
        let mut b = PrefBook::new();
        let _ = b.put(1, &DEV[..3], &DEV, "dev");
        b.judge(1, false) == Ok(true) && b.judge(9, false).is_err()
    };
    s.add("J8-档判定", j8, "按档判定且缺档拒");

    // J9 重号拒。
    let j9 = {
        let mut b = PrefBook::new();
        let _ = b.put(3, &DEV[..2], &DEV, "a");
        b.put(3, &DEV[..2], &DEV, "b").err() == Some(PF_DUP_ID)
    };
    s.add("J9-重号拒", j9, "同号二次必拒");

    // J10 回读渲染。
    let j10 = {
        let mut b = PrefBook::new();
        let _ = b.put(5, &DEV[..3], &DEV, "p");
        b.line_of(5).contains("p3|s8") && b.line_of(6) == "missing"
    };
    s.add("J10-回读渲染", j10, "档行含双长");

    // J11 印账冻结。
    let j11 = {
        let mut b = PrefBook::new();
        b.seal();
        b.put(1, &DEV[..2], &DEV, "p").err() == Some(PF_FROZEN)
    };
    s.add("J11-印账冻结", j11, "seal 后必拒");

    // J12 指纹幂等。
    let j12 = {
        let mut b = PrefBook::new();
        let _ = b.put(1, &DEV[..2], &DEV, "a");
        b.fold() == b.fold()
    };
    s.add("J12-指纹幂等", j12, "同态同指纹");

    // J13 注入核 5/5。
    let got = inject_five();
    let j13 = got.iter().all(|x| *x);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = PrefBook::new();
    let _ = b.put(1, &DEV[..3], &DEV, "dev-prefix");
    let _ = b.put(2, &DEV[..6], &DEV, "device-prefix");

    s.add("摘要回读", b.summary().contains("recs=2"), "双档在册");
    s.add("判定回读", b.judge(2, true) == Ok(true), "device 前缀命中");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12111_prefix_flow() {
        let s = [0x64u16, 0x65, 0x76];
        assert_eq!(prefix_check(&s[..2], &s, false), Ok(true));
        assert_eq!(prefix_check(&[], &s, false), Err(PF_EMPTY));
        assert_eq!(prefix_check(&s, &s[..2], false), Err(PF_LONGER));
    }

    #[test]
    fn unx12111_checks_green() {
        let s = run_unx12111_checks();
        assert!(s.all_passed(), "UNX-F12111 判据未全绿");
    }

    #[test]
    fn insensitive_prefix() {
        let s = [0x44u16, 0x45, 0x56];
        assert_eq!(prefix_check(&[0x64, 0x65, 0x76], &s, true), Ok(true));
        assert_eq!(prefix_check(&[0x64, 0x65, 0x76], &s, false), Ok(false));
    }
}
