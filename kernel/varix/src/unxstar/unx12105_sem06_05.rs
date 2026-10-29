//! UNX-F12105 RtlCopyUnicodeString 语义（0x19f40 段）
//!
//! 卡面：RtlCopyUnicodeString——源复制入目标：目标容量不足时截断
//! （截断次数显性记账）、NUL 位留容约束、零长源合法（清空目标）。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 目标在场：未登记目标必拒。
//! - J2 NUL 位约束：目标 max < 2（无 NUL 容）必拒。
//! - J3 源长口径：源长 > 0xFFFE 必拒。
//! - J4 截断显性：源长 > 目标自由容时截断至 max − 2 并记截断一次。
//! - J5 零长源：源长 0 时目标清空（len 归零）。
//! - J6 印账冻结：seal 后复制必拒。
//! - J7 注入五况全抓。
//!
//! 结构：CopyDest 目标档 + CopyBook 目标册 + 复制核 + 折叠指纹
//! （211 进制卷积）+ 注入核（无目标/无 NUL 容/源越限/截断入账/印后复制）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19f40 独占 0x20）。
pub const SEG_BASE: u32 = 0x19f40;
pub const CP_NO_DEST: u32 = SEG_BASE + 0x01; // 无目标
pub const CP_NO_NUL: u32 = SEG_BASE + 0x02; // 无 NUL 容
pub const CP_SRC_OVER: u32 = SEG_BASE + 0x03; // 源越限
pub const CP_ZERO_ID: u32 = SEG_BASE + 0x04; // 零号
pub const CP_FROZEN: u32 = SEG_BASE + 0x05; // 印账后复制

/// 源长上限。
pub const CP_SRC_CAP: u32 = 0xFFFE;

/// 目标档。
#[derive(Clone, Copy)]
pub struct CopyDest {
    pub id: u32,
    pub len: u32,
    pub max: u32,
    pub truncations: u32,
}

impl CopyDest {
    pub fn free(&self) -> u32 {
        if self.max >= self.len + 2 {
            self.max - self.len - 2
        } else {
            0
        }
    }
    pub fn render(&self) -> String {
        format!("D{}|{}+{}|t{}", self.id, self.len, self.max, self.truncations)
    }
}

/// 目标册。
pub struct CopyBook {
    pub dests: Vec<CopyDest>,
    pub sealed: bool,
}

impl CopyBook {
    pub fn new() -> CopyBook {
        CopyBook { dests: Vec::new(), sealed: false }
    }

    /// 登记目标：编号唯一（含零号）与 NUL 容约束。
    pub fn add_dest(&mut self, id: u32, max: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CP_FROZEN);
        }
        if id == 0 {
            return Err(CP_ZERO_ID);
        }
        if max < 2 {
            return Err(CP_NO_NUL);
        }
        if self.dests.iter().any(|d| d.id == id) {
            return Err(CP_ZERO_ID); // 重目标按零面拒
        }
        self.dests.push(CopyDest { id, len: 0, max, truncations: 0 });
        Ok(())
    }

    fn pos(&self, id: u32) -> Option<usize> {
        self.dests.iter().position(|d| d.id == id)
    }

    /// 复制核：无目标 → 源越限 → 截断/直放。
    pub fn copy(&mut self, id: u32, src_len: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(CP_FROZEN);
        }
        if src_len > CP_SRC_CAP {
            return Err(CP_SRC_OVER);
        }
        let i = match self.pos(id) {
            Some(i) => i,
            None => return Err(CP_NO_DEST),
        };
        if src_len == 0 {
            self.dests[i].len = 0;
            return Ok(false);
        }
        let free = self.dests[i].free();
        if src_len <= free {
            self.dests[i].len += src_len;
            Ok(false)
        } else {
            // 截断至 NUL 位前。
            self.dests[i].len = self.dests[i].max - 2;
            self.dests[i].truncations += 1;
            Ok(true)
        }
    }

    /// 截断合计。
    pub fn trunc_sum(&self) -> u32 {
        self.dests.iter().map(|d| d.truncations).sum()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for d in &self.dests {
            h = h.wrapping_mul(211).wrapping_add(d.id);
            h = h.wrapping_mul(211).wrapping_add(d.len);
            h = h.wrapping_mul(211).wrapping_add(d.max);
            h = h.wrapping_mul(211).wrapping_add(d.truncations);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("dests={} trunc={}", self.dests.len(), self.trunc_sum())
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
    let mut b = CopyBook::new();
    let _ = b.add_dest(4, 16);
    // 1) 无目标必拒。
    got[0] = b.copy(9, 4).err() == Some(CP_NO_DEST);
    // 2) 无 NUL 容必拒（max=1）。
    got[1] = b.add_dest(5, 1).err() == Some(CP_NO_NUL);
    // 3) 源越限必拒。
    got[2] = b.copy(4, 0x1_0000).err() == Some(CP_SRC_OVER);
    // 4) 截断入账（自由容 14 放 16 字节 → 截断一次）。
    let _ = b.copy(4, 16);
    got[3] = b.trunc_sum() == 1
        && b.dests[0].len == 14
        && b.dests[0].render().contains("t1");
    // 5) 印账后复制必抓。
    b.seal();
    got[4] = b.copy(4, 2).err() == Some(CP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12105_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12105");

    // J1 无目标拒。
    let j1 = {
        let mut b = CopyBook::new();
        b.copy(1, 4).err() == Some(CP_NO_DEST)
    };
    s.add("J1-无目标拒", j1, "未登记目标必拒");

    // J2 无 NUL 容拒。
    let j2 = {
        let mut b = CopyBook::new();
        b.add_dest(1, 0).err() == Some(CP_NO_NUL)
            && b.add_dest(1, 1).err() == Some(CP_NO_NUL)
    };
    s.add("J2-无NUL容拒", j2, "max<2 必拒");

    // J3 源越限拒。
    let j3 = {
        let mut b = CopyBook::new();
        let _ = b.add_dest(1, 64);
        b.copy(1, 0xFFFF).err() == Some(CP_SRC_OVER)
    };
    s.add("J3-源越限拒", j3, "源长越 0xFFFE 必拒");

    // J4 足容直放。
    let j4 = {
        let mut b = CopyBook::new();
        let _ = b.add_dest(1, 64);
        let trunc = b.copy(1, 8).unwrap_or(true);
        !trunc && b.dests[0].len == 8
    };
    s.add("J4-足容直放", j4, "无截断入账");

    // J5 截断显性。
    let j5 = {
        let mut b = CopyBook::new();
        let _ = b.add_dest(1, 16);
        let trunc = b.copy(1, 20).unwrap_or(false);
        trunc && b.trunc_sum() == 1 && b.dests[0].len == 14
    };
    s.add("J5-截断显性", j5, "截断至 max−2 且入账");

    // J6 零长源清空。
    let j6 = {
        let mut b = CopyBook::new();
        let _ = b.add_dest(1, 16);
        let _ = b.copy(1, 8);
        let _ = b.copy(1, 0);
        b.dests[0].len == 0
    };
    s.add("J6-零长清空", j6, "len 归零");

    // J7 零号目标拒。
    let j7 = {
        let mut b = CopyBook::new();
        b.add_dest(0, 16).err() == Some(CP_ZERO_ID)
    };
    s.add("J7-零号拒", j7, "零号必拒");

    // J8 回读渲染。
    let j8 = {
        let mut b = CopyBook::new();
        let _ = b.add_dest(3, 32);
        b.line_of(3).contains("D3|0+32") && b.line_of(4) == "missing"
    };
    s.add("J8-回读渲染", j8, "目标行可渲染");

    // J9 印账冻结。
    let j9 = {
        let mut b = CopyBook::new();
        let _ = b.add_dest(1, 16);
        b.seal();
        b.copy(1, 2).err() == Some(CP_FROZEN)
            && b.add_dest(2, 16).err() == Some(CP_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后皆拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = CopyBook::new();
        let _ = b.add_dest(1, 16);
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = CopyBook::new();
    let _ = b.add_dest(1, 16);
    let _ = b.copy(1, 6);
    let _ = b.add_dest(2, 8);
    let _ = b.copy(2, 20);

    s.add("摘要回读", b.summary().contains("dests=2 trunc=1"), "一截断在账");
    s.add("指纹非零", b.fold() != 0, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12105_copy_flow() {
        let mut b = CopyBook::new();
        let _ = b.add_dest(1, 16);
        assert!(b.copy(1, 4).is_ok());
        assert_eq!(b.copy(9, 4), Err(CP_NO_DEST));
        let t = b.copy(1, 40).unwrap_or(false);
        assert!(t);
        assert_eq!(b.dests[0].len, 14);
    }

    #[test]
    fn unx12105_checks_green() {
        let s = run_unx12105_checks();
        assert!(s.all_passed(), "UNX-F12105 判据未全绿");
    }

    #[test]
    fn free_space_helper() {
        let d = CopyDest { id: 1, len: 4, max: 16, truncations: 0 };
        assert_eq!(d.free(), 10);
        let bad = CopyDest { id: 2, len: 20, max: 16, truncations: 0 };
        assert_eq!(bad.free(), 0);
    }
}
