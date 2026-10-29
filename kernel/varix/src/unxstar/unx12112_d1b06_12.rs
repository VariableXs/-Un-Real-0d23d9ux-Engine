//! UNX-F12112 RtlIntegerToUnicodeString/整数双向转换族（0x1a020 段）
//!
//! 卡面：整数↔串双向转换——进制闭集 {2,8,10,16}、位数容量预检、
//! 零值特例（"0"）、往返一致性（转出再转入还原）。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 进制闭集：基数仅 2/8/10/16，越集必拒。
//! - J2 位数容量：目标 max 须容 digits×2 + 2（NUL 位），否则必拒。
//! - J3 目标在场：未登记目标必拒。
//! - J4 零值特例：0 转出恰为单字符 "0"。
//! - J5 往返一致：u32 → 串 → u32 还原（含边界 0/0xFFFFFFFF）。
//! - J6 印账冻结：seal 后转换必拒。
//! - J7 注入五况全抓。
//!
//! 结构：IntDest 转换目标档 + IntBook 目标册 + 转换核/回转核 +
//! 折叠指纹（211 进制卷积）+ 注入核（越进制/容不足/无目标/零号/印后转换）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a020 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a020;
pub const IT_BAD_BASE: u32 = SEG_BASE + 0x01; // 越进制
pub const IT_NO_ROOM: u32 = SEG_BASE + 0x02; // 容不足
pub const IT_NO_DEST: u32 = SEG_BASE + 0x03; // 无目标
pub const IT_FROZEN: u32 = SEG_BASE + 0x04; // 印账后转换

/// 进制闭集判定。
pub fn base_ok(base: u32) -> bool {
    matches!(base, 2 | 8 | 10 | 16)
}

/// u32 十六进制字符。
fn digit_char(v: u32) -> u16 {
    if v < 10 {
        0x30 + v as u16
    } else {
        0x41 + (v - 10) as u16
    }
}

/// u32 → 单元序列（指定进制，不含 NUL）。
pub fn to_units(mut val: u32, base: u32) -> Result<Vec<u16>, u32> {
    if !base_ok(base) {
        return Err(IT_BAD_BASE);
    }
    if val == 0 {
        return Ok(vec![0x30]);
    }
    let mut out = Vec::new();
    while val > 0 {
        out.push(digit_char(val % base));
        val /= base;
    }
    out.reverse();
    Ok(out)
}

/// 单元序列 → u32（指定进制，往返核）。
pub fn from_units(units: &[u16], base: u32) -> Result<u32, u32> {
    if !base_ok(base) {
        return Err(IT_BAD_BASE);
    }
    if units.is_empty() {
        return Err(IT_NO_DEST); // 空串视作无目标面
    }
    let mut acc: u32 = 0;
    for u in units {
        let d = match u {
            0x30..=0x39 => (*u - 0x30) as u32,
            0x41..=0x5A => (*u - 0x41 + 10) as u32,
            0x61..=0x7A => (*u - 0x61 + 10) as u32,
            _ => return Err(IT_BAD_BASE),
        };
        if d >= base {
            return Err(IT_BAD_BASE);
        }
        acc = acc.wrapping_mul(base).wrapping_add(d);
    }
    Ok(acc)
}

/// 转换目标档。
#[derive(Clone, Copy)]
pub struct IntDest {
    pub id: u32,
    pub max: u32,
    pub len: u32,
}

/// 目标册。
pub struct IntBook {
    pub dests: Vec<IntDest>,
    pub sealed: bool,
}

impl IntBook {
    pub fn new() -> IntBook {
        IntBook { dests: Vec::new(), sealed: false }
    }

    /// 登记目标。
    pub fn add_dest(&mut self, id: u32, max: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IT_FROZEN);
        }
        if id == 0 {
            return Err(IT_NO_DEST);
        }
        if self.dests.iter().any(|d| d.id == id) {
            return Err(IT_NO_DEST);
        }
        self.dests.push(IntDest { id, max, len: 0 });
        Ok(())
    }

    fn pos(&self, id: u32) -> Option<usize> {
        self.dests.iter().position(|d| d.id == id)
    }

    /// 转换核：进制 → 目标在场 → 容足 → 长度入账。
    pub fn convert(&mut self, id: u32, val: u32, base: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(IT_FROZEN);
        }
        let units = to_units(val, base)?;
        let i = match self.pos(id) {
            Some(i) => i,
            None => return Err(IT_NO_DEST),
        };
        let need = units.len() as u32 * 2 + 2;
        if need > self.dests[i].max {
            return Err(IT_NO_ROOM);
        }
        self.dests[i].len = units.len() as u32 * 2;
        Ok(units.len())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for d in &self.dests {
            h = h.wrapping_mul(211).wrapping_add(d.id);
            h = h.wrapping_mul(211).wrapping_add(d.max);
            h = h.wrapping_mul(211).wrapping_add(d.len);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("dests={}", self.dests.len())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, id: u32) -> String {
        match self.dests.iter().find(|d| d.id == id) {
            Some(d) => format!("T{}|{}+{}", d.id, d.len, d.max),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = IntBook::new();
    let _ = b.add_dest(4, 64);
    // 1) 越进制必拒。
    got[0] = b.convert(4, 255, 3).err() == Some(IT_BAD_BASE);
    // 2) 容不足必拒。
    let _ = b.add_dest(5, 4);
    got[1] = b.convert(5, 0xFFFF, 10).err() == Some(IT_NO_ROOM);
    // 3) 无目标必拒。
    got[2] = b.convert(9, 1, 10).err() == Some(IT_NO_DEST);
    // 4) 零号目标必拒。
    got[3] = b.add_dest(0, 16).err() == Some(IT_NO_DEST);
    // 5) 印账后转换必抓。
    b.seal();
    got[4] = b.convert(4, 1, 10).err() == Some(IT_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12112_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12112");

    // J1 进制闭集。
    let j1 = !base_ok(3) && !base_ok(0) && base_ok(2) && base_ok(16);
    s.add("J1-进制闭集", j1, "2/8/10/16 四值");

    // J2 零值特例。
    let j2 = to_units(0, 10) == Ok(vec![0x30]);
    s.add("J2-零值特例", j2, "0 → \"0\"");

    // J3 十进制转出。
    let j3 = {
        let u = to_units(12345, 10).unwrap_or_default();
        u.len() == 5 && u[0] == 0x31 && u[4] == 0x35
    };
    s.add("J3-十进制转出", j3, "12345 → 5 位");

    // J4 十六进制转出。
    let j4 = {
        let u = to_units(0xABCD, 16).unwrap_or_default();
        u.len() == 4 && u[0] == 0x41 && u[3] == 0x44
    };
    s.add("J4-十六进制", j4, "ABCD 大写口径");

    // J5 往返一致（含边界）。
    let j5 = {
        let cases: [u32; 5] = [0, 1, 0x7FFF_FFFF, 0xFFFF_FFFF, 0x1234_5678];
        cases.iter().all(|v| {
            let u = match to_units(*v, 16) {
                Ok(u) => u,
                Err(_) => return false,
            };
            from_units(&u, 16) == Ok(*v)
        })
    };
    s.add("J5-往返一致", j5, "五值全还原");

    // J6 回转坏字符拒。
    let j6 = from_units(&[0x47u16], 10).is_err() // 'G' 非十进制
        && from_units(&[0x32u16], 2).is_err(); // 2 越二进制
    s.add("J6-回转拒", j6, "越字符/越进制皆拒");

    // J7 目标容足放行。
    let j7 = {
        let mut b = IntBook::new();
        let _ = b.add_dest(1, 16);
        b.convert(1, 999, 10) == Ok(3) && b.dests[0].len == 6
    };
    s.add("J7-容足放行", j7, "len=6 入账");

    // J8 容不足拒。
    let j8 = {
        let mut b = IntBook::new();
        let _ = b.add_dest(1, 4);
        b.convert(1, 999, 10).err() == Some(IT_NO_ROOM)
    };
    s.add("J8-容不足拒", j8, "need 8 > max 4 必拒");

    // J9 无目标拒。
    let j9 = {
        let mut b = IntBook::new();
        b.convert(9, 1, 10).err() == Some(IT_NO_DEST)
    };
    s.add("J9-无目标拒", j9, "未登记必拒");

    // J10 回读渲染。
    let j10 = {
        let mut b = IntBook::new();
        let _ = b.add_dest(3, 32);
        b.line_of(3).contains("T3|0+32") && b.line_of(4) == "missing"
    };
    s.add("J10-回读渲染", j10, "目标行可渲染");

    // J11 印账冻结。
    let j11 = {
        let mut b = IntBook::new();
        let _ = b.add_dest(1, 16);
        b.seal();
        b.convert(1, 1, 10).err() == Some(IT_FROZEN)
    };
    s.add("J11-印账冻结", j11, "seal 后必拒");

    // J12 指纹幂等。
    let j12 = {
        let mut b = IntBook::new();
        let _ = b.add_dest(1, 16);
        b.fold() == b.fold()
    };
    s.add("J12-指纹幂等", j12, "同态同指纹");

    // J13 注入核 5/5。
    let got = inject_five();
    let j13 = got.iter().all(|x| *x);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = IntBook::new();
    let _ = b.add_dest(1, 32);
    let _ = b.convert(1, 0xDEAD, 16);
    let _ = b.add_dest(2, 16);
    let _ = b.convert(2, 0, 10);

    s.add("摘要回读", b.summary().contains("dests=2"), "双目标在册");
    s.add("长度分账", b.dests[0].len == 8 && b.dests[1].len == 2, "DEAD 8/0 2");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12112_roundtrip() {
        let u = to_units(48879, 16).unwrap_or_default();
        assert_eq!(from_units(&u, 16), Ok(48879));
        assert!(to_units(1, 7).is_err());
    }

    #[test]
    fn unx12112_checks_green() {
        let s = run_unx12112_checks();
        assert!(s.all_passed(), "UNX-F12112 判据未全绿");
    }

    #[test]
    fn book_convert_flow() {
        let mut b = IntBook::new();
        let _ = b.add_dest(1, 16);
        assert_eq!(b.convert(1, 42, 10), Ok(2));
        assert_eq!(b.convert(9, 1, 10), Err(IT_NO_DEST));
    }
}
