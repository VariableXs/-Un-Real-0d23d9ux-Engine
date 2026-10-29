//! UNX-F12082 OBJ_CASE_INSENSITIVE 位语义（0x19c60 段）
//!
//! 卡面：OBJ_CASE_INSENSITIVE 位——打开/查询时名字匹配按大小写不敏感
//! 折叠；折叠后二义（多条目同键）显性拒；敏感模式逐字节精确匹配。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 敏感精确：逐字节比较，大小写不同即不匹配。
//! - J2 不敏感折叠：ASCII a-z 折叠到 A-Z 后等值即命中。
//! - J3 二义显性：不敏感模式下多键命中必拒。
//! - J4 空名拒：空名查询必拒。
//! - J5 条目唯一：同字节串重复登记必拒。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：NameTable 条目册 + ASCII 折叠函数 + 查找核（敏感/不敏感双模）+
//! 折叠指纹（211 进制卷积）+ 注入核（空名/重目/未中/二义/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19c60 独占 0x20）。
pub const SEG_BASE: u32 = 0x19c60;
pub const CI_EMPTY: u32 = SEG_BASE + 0x01; // 空名
pub const CI_DUP_ENTRY: u32 = SEG_BASE + 0x02; // 重目
pub const CI_NO_MATCH: u32 = SEG_BASE + 0x03; // 未中
pub const CI_AMBIG: u32 = SEG_BASE + 0x04; // 二义
pub const CI_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// ASCII 大小折叠：a-z → A-Z。
pub fn upcase_ascii(b: u8) -> u8 {
    if b >= b'a' && b <= b'z' {
        b - 0x20
    } else {
        b
    }
}

/// 名字表。
pub struct NameTable {
    pub entries: Vec<String>,
    pub sealed: bool,
}

impl NameTable {
    pub fn new() -> NameTable {
        NameTable { entries: Vec::new(), sealed: false }
    }

    /// 登记：同字节串重复必拒。
    pub fn add(&mut self, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CI_FROZEN);
        }
        if self.entries.iter().any(|e| e.as_str() == name) {
            return Err(CI_DUP_ENTRY);
        }
        self.entries.push(name.to_string());
        Ok(())
    }

    /// 查找核：敏感逐字节；不敏感折叠后判重数。
    pub fn lookup(&self, name: &str, insensitive: bool) -> Result<usize, u32> {
        if name.is_empty() {
            return Err(CI_EMPTY);
        }
        let mut hits: Vec<usize> = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            let hit = if insensitive {
                e.len() == name.len()
                    && e.bytes()
                        .zip(name.bytes())
                        .all(|(a, b)| upcase_ascii(a) == upcase_ascii(b))
            } else {
                e.as_str() == name
            };
            if hit {
                hits.push(i);
            }
        }
        if hits.len() > 1 {
            return Err(CI_AMBIG);
        }
        if hits.is_empty() {
            return Err(CI_NO_MATCH);
        }
        Ok(hits[0])
    }

    /// 条目数。
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for e in &self.entries {
            for by in e.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
            h = h.wrapping_mul(211).wrapping_add(0x7E);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("entries={}", self.entries.len())
    }

    /// 诊断面：条目回读。
    pub fn entry_of(&self, i: usize) -> String {
        match self.entries.get(i) {
            Some(e) => e.clone(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = NameTable::new();
    let _ = t.add("Abc");
    // 1) 空名必拒。
    got[0] = t.lookup("", true).err() == Some(CI_EMPTY);
    // 2) 重目必拒。
    got[1] = t.add("Abc").err() == Some(CI_DUP_ENTRY);
    // 3) 未中必拒。
    got[2] = t.lookup("Xyz", true).err() == Some(CI_NO_MATCH);
    // 4) 二义必拒：Abc 与 aBC 折叠同键。
    let _ = t.add("aBC");
    got[3] = t.lookup("ABC", true).err() == Some(CI_AMBIG);
    // 5) 印账后登记必抓。
    t.seal();
    got[4] = t.add("Post").err() == Some(CI_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12082_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12082");

    // J1 敏感精确：大小写不同不中。
    let j1 = {
        let mut t = NameTable::new();
        let _ = t.add("ABC");
        t.lookup("abc", false).err() == Some(CI_NO_MATCH)
            && t.lookup("ABC", false).is_ok()
    };
    s.add("J1-敏感精确", j1, "逐字节精确");

    // J2 不敏感折叠命中。
    let j2 = {
        let mut t = NameTable::new();
        let _ = t.add("Device");
        t.lookup("DEVICE", true).is_ok() && t.lookup("device", true).is_ok()
    };
    s.add("J2-不敏感命中", j2, "双向折叠等值");

    // J3 二义显性。
    let j3 = {
        let mut t = NameTable::new();
        let _ = t.add("obj");
        let _ = t.add("OBJ");
        t.lookup("Obj", true).err() == Some(CI_AMBIG)
    };
    s.add("J3-二义显性", j3, "折叠同键必拒");

    // J4 敏感模式同键不二义。
    let j4 = {
        let mut t = NameTable::new();
        let _ = t.add("obj");
        let _ = t.add("OBJ");
        t.lookup("obj", false).is_ok()
    };
    s.add("J4-敏感不折叠", j4, "敏感模式无二义");

    // J5 空名拒。
    let j5 = {
        let mut t = NameTable::new();
        t.lookup("", true).err() == Some(CI_EMPTY)
    };
    s.add("J5-空名拒", j5, "空名必拒");

    // J6 重目拒。
    let j6 = {
        let mut t = NameTable::new();
        let _ = t.add("X");
        t.add("X").err() == Some(CI_DUP_ENTRY)
    };
    s.add("J6-重目拒", j6, "同串二次必拒");

    // J7 折叠函数闭包：自反。
    let j7 = upcase_ascii(b'q') == b'Q'
        && upcase_ascii(b'Q') == b'Q'
        && upcase_ascii(b'1') == b'1';
    s.add("J7-折叠自反", j7, "小写升大写、其余恒等");

    // J8 未中显性。
    let j8 = {
        let mut t = NameTable::new();
        t.lookup("nope", false).err() == Some(CI_NO_MATCH)
    };
    s.add("J8-未中显性", j8, "空表亦未中");

    // J9 印账冻结。
    let j9 = {
        let mut t = NameTable::new();
        t.seal();
        t.add("Post").err() == Some(CI_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut t = NameTable::new();
        let _ = t.add("A");
        t.fold() == t.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut t = NameTable::new();
    let _ = t.add("Alpha");
    let _ = t.add("Beta");

    s.add("摘要回读", t.summary().contains("entries=2"), "双目在册");
    s.add("条目回读", t.entry_of(0) == "Alpha", "首目可回读");
    s.add("缺席行", t.entry_of(9) == "missing", "缺目显性");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12082_lookup_flow() {
        let mut t = NameTable::new();
        let _ = t.add("Key");
        assert!(t.lookup("KEY", true).is_ok());
        assert_eq!(t.lookup("key", false), Err(CI_NO_MATCH));
        assert_eq!(t.add("Key"), Err(CI_DUP_ENTRY));
    }

    #[test]
    fn unx12082_checks_green() {
        let s = run_unx12082_checks();
        assert!(s.all_passed(), "UNX-F12082 判据未全绿");
    }

    #[test]
    fn upcase_helper() {
        assert_eq!(upcase_ascii(b'z'), b'Z');
        assert_eq!(upcase_ascii(b'a'), b'A');
        assert_eq!(upcase_ascii(0x00), 0x00);
    }
}
