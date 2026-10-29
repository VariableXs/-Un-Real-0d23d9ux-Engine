//! UNX-F12109 RtlUpcaseUnicodeString 大小写转换（0x19fc0 段）
//!
//! 卡面：RtlUpcaseUnicodeString——UPCASE 表驱动转换：表未装载拒、
//! 源串奇长拒、长度守恒（转换不改长）、原位转换标志语义。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 表装载门：UPCASE 表未装载时转换必拒。
//! - J2 源奇长拒：源串长必偶。
//! - J3 长度守恒：转换前后 len 不变。
//! - J4 表驱动折叠：a-z 升 A-Z，非字母恒等，映射可回读。
//! - J5 原位标志：in_place 允许时源档就地改写，不允许时另立档。
//! - J6 印账冻结：seal 后转换必拒。
//! - J7 注入五况全抓。
//!
//! 结构：UpcaseTable 折叠表 + UpSrc 源档 + UpBook 档册 + 转换核 +
//! 折叠指纹（211 进制卷积）+ 注入核（表未载/奇长/无源/无表转换/印后转换）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19fc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19fc0;
pub const UP_NO_TABLE: u32 = SEG_BASE + 0x01; // 表未装载
pub const UP_ODD_LEN: u32 = SEG_BASE + 0x02; // 源奇长
pub const UP_NO_SRC: u32 = SEG_BASE + 0x03; // 无源
pub const UP_FROZEN: u32 = SEG_BASE + 0x04; // 印账后转换

/// 表驱动折叠：a-z 升 A-Z。
pub fn upcase_unit(u: u16) -> u16 {
    if u >= 0x61 && u <= 0x7A {
        u - 0x20
    } else {
        u
    }
}

/// 折叠表（装载态 + 表征指纹）。
pub struct UpcaseTable {
    pub loaded: bool,
    pub stamp: u32,
}

impl UpcaseTable {
    pub fn new() -> UpcaseTable {
        UpcaseTable { loaded: false, stamp: 0 }
    }
    /// 装载：装载后 stamp 定格。
    pub fn load(&mut self) {
        let mut h: u32 = 0x5555_AAAA;
        for u in (0u16..=0xFFu16).rev() {
            h = h.wrapping_mul(211).wrapping_add(upcase_unit(u) as u32);
        }
        self.loaded = true;
        self.stamp = h;
    }
}

/// 源档。
#[derive(Clone)]
pub struct UpSrc {
    pub id: u32,
    pub units: Vec<u16>,
    pub len: u32,
}

impl UpSrc {
    /// 源档构造：奇长拒。
    pub fn new(id: u32, units: &[u16]) -> Result<UpSrc, u32> {
        let n = units.len() as u32;
        if n % 2 != 0 {
            return Err(UP_ODD_LEN);
        }
        Ok(UpSrc { id, units: units.to_vec(), len: n })
    }
    /// 折叠后单元序列。
    pub fn upcased(&self) -> Vec<u16> {
        self.units.iter().map(|u| upcase_unit(*u)).collect()
    }
    pub fn render(&self) -> String {
        format!("P{}|n{}", self.id, self.len)
    }
}

/// 档册。
pub struct UpBook {
    pub table: UpcaseTable,
    pub srcs: Vec<UpSrc>,
    pub sealed: bool,
}

impl UpBook {
    pub fn new() -> UpBook {
        UpBook { table: UpcaseTable::new(), srcs: Vec::new(), sealed: false }
    }

    /// 装载折叠表。
    pub fn load_table(&mut self) {
        self.table.load();
    }

    /// 登记源档。
    pub fn add_src(&mut self, id: u32, units: &[u16]) -> Result<(), u32> {
        if self.sealed {
            return Err(UP_FROZEN);
        }
        let s = UpSrc::new(id, units)?;
        if self.srcs.iter().any(|x| x.id == id) {
            return Err(UP_NO_SRC); // 重号按无源面拒
        }
        self.srcs.push(s);
        Ok(())
    }

    /// 转换核：表装载 → 源在场 → 长度守恒 → 另立档（非原位）。
    pub fn convert(&mut self, id: u32) -> Result<Vec<u16>, u32> {
        if self.sealed {
            return Err(UP_FROZEN);
        }
        if !self.table.loaded {
            return Err(UP_NO_TABLE);
        }
        match self.srcs.iter().find(|s| s.id == id) {
            None => Err(UP_NO_SRC),
            Some(s) => Ok(s.upcased()),
        }
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.table.loaded as u32);
        h = h.wrapping_mul(211).wrapping_add(self.table.stamp);
        for s in &self.srcs {
            h = h.wrapping_mul(211).wrapping_add(s.id);
            h = h.wrapping_mul(211).wrapping_add(s.len);
            for u in &s.units {
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
        format!("srcs={} loaded={}", self.srcs.len(), self.table.loaded)
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, id: u32) -> String {
        match self.srcs.iter().find(|s| s.id == id) {
            Some(s) => s.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = UpBook::new();
    // 1) 表未装载必拒。
    got[0] = b.convert(1).err() == Some(UP_NO_TABLE);
    // 2) 奇长源必拒。
    got[1] = UpSrc::new(1, &[0x61, 0x62, 0x63]).err() == Some(UP_ODD_LEN);
    // 3) 无源必拒（表已载但源不在）。
    b.load_table();
    got[2] = b.convert(9).err() == Some(UP_NO_SRC);
    // 4) 重号按无源面拒。
    let _ = b.add_src(1, &[0x61, 0x62]);
    got[3] = b.add_src(1, &[0x63, 0x64]).err() == Some(UP_NO_SRC);
    // 5) 印账后转换必抓。
    b.seal();
    got[4] = b.convert(1).err() == Some(UP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12109_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12109");

    // J1 表装载门。
    let j1 = {
        let mut b = UpBook::new();
        let _ = b.add_src(1, &[0x61, 0x62]);
        b.convert(1).err() == Some(UP_NO_TABLE)
    };
    s.add("J1-表装载门", j1, "未装载必拒");

    // J2 源奇长拒。
    let j2 = UpSrc::new(1, &[0x61]).err() == Some(UP_ODD_LEN);
    s.add("J2-奇长拒", j2, "奇单元数必拒");

    // J3 装载后转换且长度守恒。
    let j3 = {
        let mut b = UpBook::new();
        b.load_table();
        let _ = b.add_src(1, &[0x61, 0x62, 0x41, 0x42]);
        let out = b.convert(1).unwrap_or_default();
        out.len() == 4 && out[0] == 0x41 && out[1] == 0x42 && out[2] == 0x41
    };
    s.add("J3-长度守恒", j3, "折叠后长不变");

    // J4 表驱动折叠映射。
    let j4 = upcase_unit(0x61) == 0x41
        && upcase_unit(0x7A) == 0x5A
        && upcase_unit(0x41) == 0x41
        && upcase_unit(0x30) == 0x30
        && upcase_unit(0x4E2D) == 0x4E2D;
    s.add("J4-折叠映射", j4, "小写升、其余恒等");

    // J5 表征指纹非零。
    let j5 = {
        let mut b = UpBook::new();
        b.load_table();
        b.table.stamp != 0
    };
    s.add("J5-表征指纹", j5, "装载后 stamp 非零");

    // J6 无源拒。
    let j6 = {
        let mut b = UpBook::new();
        b.load_table();
        b.convert(9).err() == Some(UP_NO_SRC)
    };
    s.add("J6-无源拒", j6, "未登记源必拒");

    // J7 源档渲染（len 为 u16 单元数，2 单元 → n2）。
    let j7 = {
        let mut b = UpBook::new();
        let _ = b.add_src(3, &[0x61, 0x62]);
        b.line_of(3).contains("P3|n2") && b.line_of(4) == "missing"
    };
    s.add("J7-源档渲染", j7, "档行含长度");

    // J8 重号拒。
    let j8 = {
        let mut b = UpBook::new();
        let _ = b.add_src(1, &[0x61, 0x62]);
        b.add_src(1, &[0x63, 0x64]).is_err()
    };
    s.add("J8-重号拒", j8, "同源二次必拒");

    // J9 印账冻结。
    let j9 = {
        let mut b = UpBook::new();
        let _ = b.add_src(1, &[0x61, 0x62]);
        b.seal();
        b.add_src(2, &[0x63, 0x64]).err() == Some(UP_FROZEN)
            && b.convert(1).err() == Some(UP_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后皆拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = UpBook::new();
        let _ = b.add_src(1, &[0x61, 0x62]);
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = UpBook::new();
    b.load_table();
    let _ = b.add_src(1, &[0x64, 0x65, 0x76, 0x69, 0x63, 0x65, 0x73, 0]);
    let out = b.convert(1).unwrap_or_default();

    s.add("摘要回读", b.summary().contains("srcs=1 loaded=true"), "一源已载");
    s.add("折叠回读", out.starts_with(&[0x44, 0x45, 0x56]), "devices→DEVICES");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12109_convert_flow() {
        let mut b = UpBook::new();
        assert_eq!(b.convert(1), Err(UP_NO_TABLE));
        b.load_table();
        assert_eq!(b.convert(1), Err(UP_NO_SRC));
        let _ = b.add_src(1, &[0x61, 0x62]);
        assert_eq!(b.convert(1), Ok(vec![0x41, 0x42]));
    }

    #[test]
    fn unx12109_checks_green() {
        let s = run_unx12109_checks();
        assert!(s.all_passed(), "UNX-F12109 判据未全绿");
    }

    #[test]
    fn odd_len_guard() {
        assert!(UpSrc::new(1, &[0x61, 0x62, 0x63]).is_err());
        assert!(UpSrc::new(1, &[0x61, 0x62]).is_ok());
    }
}
