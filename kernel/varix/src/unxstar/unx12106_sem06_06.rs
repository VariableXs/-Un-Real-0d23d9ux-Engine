//! UNX-F12106 RtlAppendUnicodeStringToString/ToString 语义（0x19f60 段）
//!
//! 卡面：RtlAppend 族——追加核：无容显性拒（BUFFER_TOO_SMALL 口径）、
//! 不做部分追加（拒则整退）、双 API（StringToString 与 StringToString
//! 的字符串对象版）共用追加核。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 目标在场：未登记目标必拒。
//! - J2 无容拒：追加须 len + add + 2 ≤ max，违者必拒且目标不漂移。
//! - J3 不做部分追加：拒时 len 原样（无半截写入）。
//! - J4 源越限拒：追加长 > 0xFFFE 必拒。
//! - J5 双 API 同核：两入口走同一追加核，行为一致。
//! - J6 印账冻结：seal 后追加必拒。
//! - J7 注入五况全抓。
//!
//! 结构：AppendDest 目标档 + AppendBook 目标册 + 追加核 + 折叠指纹
//! （211 进制卷积）+ 注入核（无目标/无容/源越限/拒后漂移核查/印后追加）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19f60 独占 0x20）。
pub const SEG_BASE: u32 = 0x19f60;
pub const AP_NO_DEST: u32 = SEG_BASE + 0x01; // 无目标
pub const AP_TOO_SMALL: u32 = SEG_BASE + 0x02; // 无容
pub const AP_SRC_OVER: u32 = SEG_BASE + 0x03; // 源越限
pub const AP_ZERO_ID: u32 = SEG_BASE + 0x04; // 零号
pub const AP_FROZEN: u32 = SEG_BASE + 0x05; // 印账后追加

/// 源长上限。
pub const AP_SRC_CAP: u32 = 0xFFFE;

/// 追加目标档。
#[derive(Clone, Copy)]
pub struct AppendDest {
    pub id: u32,
    pub len: u32,
    pub max: u32,
}

impl AppendDest {
    pub fn free(&self) -> u32 {
        if self.max >= self.len + 2 {
            self.max - self.len - 2
        } else {
            0
        }
    }
    pub fn render(&self) -> String {
        format!("A{}|{}+{}", self.id, self.len, self.max)
    }
}

/// 目标册。
pub struct AppendBook {
    pub dests: Vec<AppendDest>,
    pub sealed: bool,
}

impl AppendBook {
    pub fn new() -> AppendBook {
        AppendBook { dests: Vec::new(), sealed: false }
    }

    /// 登记目标。
    pub fn add_dest(&mut self, id: u32, max: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(AP_FROZEN);
        }
        if id == 0 {
            return Err(AP_ZERO_ID);
        }
        if max < 2 {
            return Err(AP_TOO_SMALL);
        }
        if self.dests.iter().any(|d| d.id == id) {
            return Err(AP_ZERO_ID);
        }
        self.dests.push(AppendDest { id, len: 0, max });
        Ok(())
    }

    fn pos(&self, id: u32) -> Option<usize> {
        self.dests.iter().position(|d| d.id == id)
    }

    /// 追加核（API 一）：字符串对象版。
    pub fn append_str(&mut self, id: u32, add: u32) -> Result<(), u32> {
        self.append_core(id, add)
    }

    /// 追加核（API 二）：以目标追加版——同一核。
    pub fn append_to_string(&mut self, id: u32, add: u32) -> Result<(), u32> {
        self.append_core(id, add)
    }

    fn append_core(&mut self, id: u32, add: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(AP_FROZEN);
        }
        if add > AP_SRC_CAP {
            return Err(AP_SRC_OVER);
        }
        let i = match self.pos(id) {
            Some(i) => i,
            None => return Err(AP_NO_DEST),
        };
        if self.dests[i].len + add + 2 > self.dests[i].max {
            return Err(AP_TOO_SMALL); // 拒则整退，不部分写
        }
        self.dests[i].len += add;
        Ok(())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for d in &self.dests {
            h = h.wrapping_mul(211).wrapping_add(d.id);
            h = h.wrapping_mul(211).wrapping_add(d.len);
            h = h.wrapping_mul(211).wrapping_add(d.max);
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
            Some(d) => d.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = AppendBook::new();
    let _ = b.add_dest(4, 16);
    // 1) 无目标必拒。
    got[0] = b.append_str(9, 4).err() == Some(AP_NO_DEST);
    // 2) 无容必拒且目标不漂移。
    let _ = b.append_str(4, 4);
    let before = b.dests[0].len;
    got[1] = b.append_to_string(4, 99).err() == Some(AP_TOO_SMALL)
        && b.dests[0].len == before;
    // 3) 源越限必拒。
    got[2] = b.append_str(4, 0x1_0000).err() == Some(AP_SRC_OVER);
    // 4) 零号目标必拒。
    got[3] = b.add_dest(0, 16).err() == Some(AP_ZERO_ID);
    // 5) 印账后追加必抓。
    b.seal();
    got[4] = b.append_str(4, 2).err() == Some(AP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12106_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12106");

    // J1 无目标拒。
    let j1 = {
        let mut b = AppendBook::new();
        b.append_str(1, 4).err() == Some(AP_NO_DEST)
    };
    s.add("J1-无目标拒", j1, "未登记目标必拒");

    // J2 无容拒。
    let j2 = {
        let mut b = AppendBook::new();
        let _ = b.add_dest(1, 16);
        b.append_str(1, 99).err() == Some(AP_TOO_SMALL)
    };
    s.add("J2-无容拒", j2, "BUFFER_TOO_SMALL 口径");

    // J3 拒则整退（无部分追加）。
    let j3 = {
        let mut b = AppendBook::new();
        let _ = b.add_dest(1, 16);
        let _ = b.append_str(1, 6);
        let before = b.dests[0].len;
        let _ = b.append_str(1, 98);
        b.dests[0].len == before
    };
    s.add("J3-拒则整退", j3, "len 原样不漂移");

    // J4 源越限拒。
    let j4 = {
        let mut b = AppendBook::new();
        let _ = b.add_dest(1, 64);
        b.append_str(1, 0xFFFF).err() == Some(AP_SRC_OVER)
    };
    s.add("J4-源越限拒", j4, "追加长越 0xFFFE 必拒");

    // J5 双 API 同核一致性。
    let j5 = {
        let mut b1 = AppendBook::new();
        let mut b2 = AppendBook::new();
        let _ = b1.add_dest(1, 32);
        let _ = b2.add_dest(1, 32);
        let _ = b1.append_str(1, 6);
        let _ = b2.append_to_string(1, 6);
        b1.dests[0].len == b2.dests[0].len
            && b1.fold() == b2.fold()
    };
    s.add("J5-双API同核", j5, "两入口行为一致");

    // J6 足容追加。
    let j6 = {
        let mut b = AppendBook::new();
        let _ = b.add_dest(1, 32);
        let _ = b.append_str(1, 6);
        let _ = b.append_str(1, 6);
        b.dests[0].len == 12
    };
    s.add("J6-足容追加", j6, "两笔 6+6=12");

    // J7 零号目标拒。
    let j7 = {
        let mut b = AppendBook::new();
        b.add_dest(0, 16).err() == Some(AP_ZERO_ID)
    };
    s.add("J7-零号拒", j7, "零号必拒");

    // J8 回读渲染。
    let j8 = {
        let mut b = AppendBook::new();
        let _ = b.add_dest(5, 16);
        b.line_of(5).contains("A5|0+16") && b.line_of(6) == "missing"
    };
    s.add("J8-回读渲染", j8, "目标行可渲染");

    // J9 印账冻结。
    let j9 = {
        let mut b = AppendBook::new();
        let _ = b.add_dest(1, 16);
        b.seal();
        b.append_str(1, 2).err() == Some(AP_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = AppendBook::new();
        let _ = b.add_dest(1, 16);
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = AppendBook::new();
    let _ = b.add_dest(1, 32);
    let _ = b.append_str(1, 10);
    let _ = b.append_to_string(1, 4);

    s.add("摘要回读", b.summary().contains("dests=1"), "单目标在册");
    s.add("长度回读", b.dests[0].len == 14, "10+4=14");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12106_append_flow() {
        let mut b = AppendBook::new();
        let _ = b.add_dest(1, 16);
        assert!(b.append_str(1, 4).is_ok());
        assert_eq!(b.append_str(1, 99), Err(AP_TOO_SMALL));
        assert_eq!(b.dests[0].len, 4);
    }

    #[test]
    fn unx12106_checks_green() {
        let s = run_unx12106_checks();
        assert!(s.all_passed(), "UNX-F12106 判据未全绿");
    }

    #[test]
    fn free_helper() {
        let d = AppendDest { id: 1, len: 2, max: 16 };
        assert_eq!(d.free(), 12);
    }
}
