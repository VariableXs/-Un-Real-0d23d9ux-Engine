//! UNX-F12123 RIP 二分查找算法与边界语义（0x1a180 段）
//!
//! 卡面：RIP→RUNTIME_FUNCTION 定位——表按 BeginAddress 升序构建、
//! 二分查找 Begin ≤ rip < End、无命中显性、空表显性。
//! 域：CoRun·UNX-D1-B07。
//!
//! 判据原文：
//! - J1 升序构建：乱序插入必拒。
//! - J2 表目唯一：同 Begin 二次插入必拒。
//! - J3 空表显性：空表查找必拒（显性码非静默 miss）。
//! - J4 命中区间：恰有一目含 rip（Begin ≤ rip < End）。
//! - J5 未中显性：rip 不落任何区间必拒。
//! - J6 印账冻结：seal 后查找必拒。
//! - J7 注入五况全抓。
//!
//! 结构：BsEntry 查找目 + BsTable 有序册 + 二分核 + 折叠指纹
//! （211 进制卷积）+ 注入核（乱序/重目/空表/未中/印后查找）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a180 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a180;
pub const BS_UNSORTED: u32 = SEG_BASE + 0x01; // 乱序
pub const BS_DUP: u32 = SEG_BASE + 0x02; // 重目
pub const BS_EMPTY: u32 = SEG_BASE + 0x03; // 空表
pub const BS_MISS: u32 = SEG_BASE + 0x04; // 未中
pub const BS_FROZEN: u32 = SEG_BASE + 0x05; // 印账后查找

/// 查找目。
#[derive(Clone, Copy)]
pub struct BsEntry {
    pub begin: u32,
    pub end: u32,
}

impl BsEntry {
    pub fn contains(&self, rip: u32) -> bool {
        rip >= self.begin && rip < self.end
    }
    pub fn render(&self) -> String {
        format!("[{:x},{:x})", self.begin, self.end)
    }
}

/// 有序查找册。
pub struct BsTable {
    pub entries: Vec<BsEntry>,
    pub sealed: bool,
}

impl BsTable {
    pub fn new() -> BsTable {
        BsTable { entries: Vec::new(), sealed: false }
    }

    /// 升序插入：乱序与重目必拒。
    pub fn insert(&mut self, e: BsEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(BS_FROZEN);
        }
        if e.begin >= e.end {
            return Err(BS_UNSORTED); // 逆区间按乱序面拒
        }
        if let Some(last) = self.entries.last() {
            if e.begin < last.begin {
                return Err(BS_UNSORTED);
            }
            if e.begin == last.begin {
                return Err(BS_DUP);
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 二分核：恰一目含 rip 时命中。
    pub fn find(&self, rip: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(BS_FROZEN);
        }
        if self.entries.is_empty() {
            return Err(BS_EMPTY);
        }
        let mut lo: usize = 0;
        let mut hi: usize = self.entries.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.entries[mid].begin <= rip {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == 0 {
            return Err(BS_MISS);
        }
        let idx = lo - 1;
        if self.entries[idx].contains(rip) {
            Ok(idx)
        } else {
            Err(BS_MISS)
        }
    }

    /// 表目数。
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.begin);
            h = h.wrapping_mul(211).wrapping_add(e.end);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("entries={}", self.count())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, i: usize) -> String {
        match self.entries.get(i) {
            Some(e) => e.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = BsTable::new();
    // 1) 空表查找必拒。
    got[0] = t.find(0x10).err() == Some(BS_EMPTY);
    // 2) 乱序插入必拒。
    let _ = t.insert(BsEntry { begin: 0x2000, end: 0x2100 });
    got[1] = t.insert(BsEntry { begin: 0x1000, end: 0x1100 }).err() == Some(BS_UNSORTED);
    // 3) 重目必拒。
    got[2] = t.insert(BsEntry { begin: 0x2000, end: 0x2200 }).err() == Some(BS_DUP);
    // 4) 未中必拒。
    got[3] = t.find(0x9000).err() == Some(BS_MISS);
    // 5) 印账后查找必抓。
    t.seal();
    got[4] = t.find(0x2050).err() == Some(BS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12123_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12123");

    // J1 有序构建与命中。
    let j1 = {
        let mut t = BsTable::new();
        let _ = t.insert(BsEntry { begin: 0x1000, end: 0x1100 });
        let _ = t.insert(BsEntry { begin: 0x2000, end: 0x2100 });
        t.find(0x1050) == Ok(0) && t.find(0x2050) == Ok(1)
    };
    s.add("J1-有序命中", j1, "双区间各自命中");

    // J2 空表显性。
    let j2 = {
        let t = BsTable::new();
        t.find(0x10).err() == Some(BS_EMPTY)
    };
    s.add("J2-空表显性", j2, "空表必拒");

    // J3 乱序拒。
    let j3 = {
        let mut t = BsTable::new();
        let _ = t.insert(BsEntry { begin: 0x3000, end: 0x3100 });
        t.insert(BsEntry { begin: 0x2000, end: 0x2100 }).err() == Some(BS_UNSORTED)
    };
    s.add("J3-乱序拒", j3, "降序插入必拒");

    // J4 重目拒。
    let j4 = {
        let mut t = BsTable::new();
        let _ = t.insert(BsEntry { begin: 0x1000, end: 0x1100 });
        t.insert(BsEntry { begin: 0x1000, end: 0x1200 }).err() == Some(BS_DUP)
    };
    s.add("J4-重目拒", j4, "同 Begin 必拒");

    // J5 逆区间拒。
    let j5 = {
        let mut t = BsTable::new();
        t.insert(BsEntry { begin: 0x1100, end: 0x1000 }).err() == Some(BS_UNSORTED)
    };
    s.add("J5-逆区间拒", j5, "Begin ≥ End 必拒");

    // J6 未中显性（区间前）。
    let j6 = {
        let mut t = BsTable::new();
        let _ = t.insert(BsEntry { begin: 0x2000, end: 0x2100 });
        t.find(0x1000).err() == Some(BS_MISS)
    };
    s.add("J6-未中显性", j6, "rip 在首目前必拒");

    // J7 未中显性（区间缝隙）。
    let j7 = {
        let mut t = BsTable::new();
        let _ = t.insert(BsEntry { begin: 0x1000, end: 0x1100 });
        let _ = t.insert(BsEntry { begin: 0x2000, end: 0x2100 });
        t.find(0x1800).err() == Some(BS_MISS)
    };
    s.add("J7-缝隙未中", j7, "缝隙 rip 必拒");

    // J8 区间端点口径：Begin 含、End 不含。
    let j8 = {
        let e = BsEntry { begin: 0x1000, end: 0x1100 };
        e.contains(0x1000) && !e.contains(0x1100) && e.render().contains("1000")
    };
    s.add("J8-端点口径", j8, "左闭右开");

    // J9 渲染行。
    let j9 = {
        let mut t = BsTable::new();
        let _ = t.insert(BsEntry { begin: 0x401000, end: 0x401040 });
        t.line_of(0).contains("[401000,401040)") && t.line_of(1) == "missing"
    };
    s.add("J9-渲染行", j9, "区间行可渲染");

    // J10 印账冻结。
    let j10 = {
        let mut t = BsTable::new();
        let _ = t.insert(BsEntry { begin: 0x1000, end: 0x1100 });
        t.seal();
        t.find(0x1050).err() == Some(BS_FROZEN)
            && t.insert(BsEntry { begin: 0x2000, end: 0x2100 }).err() == Some(BS_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后皆拒");

    // J11 指纹幂等。
    let j11 = {
        let mut t = BsTable::new();
        let _ = t.insert(BsEntry { begin: 0x1000, end: 0x1100 });
        t.fold() == t.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut t = BsTable::new();
    let _ = t.insert(BsEntry { begin: 0x401000, end: 0x401080 });
    let _ = t.insert(BsEntry { begin: 0x4010a0, end: 0x4010c0 });

    s.add("摘要回读", t.summary().contains("entries=2"), "双目在册");
    s.add("查找回读", t.find(0x4010b0) == Ok(1), "次目命中");

    // J13 端点未中显性：次目 Begin 不含（右开）。
    let j13 = t.find(0x4010a0) == Ok(1) && t.find(0x4010c0).err() == Some(BS_MISS);
    s.add("J13-次目端点", j13, "Begin 含 / End 不含");

    // J14 跨目命中序：首目尾缝不串目。
    let j14 = t.find(0x40107f) == Ok(0) && t.find(0x401080).err() == Some(BS_MISS);
    s.add("J14-跨目不串", j14, "缝隙两端各归各");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12123_bs_flow() {
        let mut t = BsTable::new();
        let _ = t.insert(BsEntry { begin: 0x1000, end: 0x1100 });
        assert_eq!(t.insert(BsEntry { begin: 0x0900, end: 0x0A00 }), Err(BS_UNSORTED));
        assert_eq!(t.find(0x1000), Ok(0));
        assert_eq!(t.find(0x1100), Err(BS_MISS));
    }

    #[test]
    fn unx12123_checks_green() {
        let s = run_unx12123_checks();
        assert!(s.all_passed(), "UNX-F12123 判据未全绿");
    }

    #[test]
    fn contains_endpoint() {
        let e = BsEntry { begin: 0x10, end: 0x20 };
        assert!(e.contains(0x1F));
        assert!(!e.contains(0x20));
    }
}
