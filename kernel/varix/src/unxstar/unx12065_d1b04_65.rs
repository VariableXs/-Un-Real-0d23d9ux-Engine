//! UNX-F12065 ThreadLocalStoragePointer TLS 槽位（0x19A40 段）
//!
//! 判据原文：ThreadLocalStoragePointer 是 TEB 中的 TLS 指针表
//! （64 槽 × 8 字节）；语义规则：槽索引 0..64；分配（TlsAlloc）
//! 返回最小空闲槽；置值/取值按槽索引；越界拒；重复分配同槽拒
//! （直至释放）；释放后槽位复用；封存后拒动作。
//!
//! 结构：槽表（TlsTable 位图 + 值阵）+ 四码守卫 + 折叠指纹 +
//! 印账冻结 + 注入核 + 判据聚合（run_unx12065_checks）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19A40 独占 0x20）。
pub const SEG_BASE: u32 = 0x19A40;
pub const TLS_RANGE: u32 = SEG_BASE + 0x01; // 槽索引越界
pub const TLS_NOFREE: u32 = SEG_BASE + 0x02; // 无空闲槽
pub const TLS_FROZEN: u32 = SEG_BASE + 0x03; // 印账后仍动作
pub const TLS_DUPFREE: u32 = SEG_BASE + 0x04; // 双重释放
pub const TLS_UNSET: u32 = SEG_BASE + 0x05; // 读未分配槽

/// TLS 槽数（64 位）。
pub const TLS_SLOTS: u32 = 64;

/// TLS 槽表。
pub struct TlsTable {
    pub used: Vec<bool>,
    pub vals: Vec<u64>,
    pub sealed: bool,
}

impl TlsTable {
    pub fn new() -> TlsTable {
        let mut used = Vec::new();
        let mut vals = Vec::new();
        let mut i = 0;
        while i < TLS_SLOTS {
            used.push(false);
            vals.push(0);
            i += 1;
        }
        TlsTable { used, vals, sealed: false }
    }

    /// 分配最小空闲槽。
    pub fn alloc(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(TLS_FROZEN);
        }
        let mut i = 0;
        while i < TLS_SLOTS {
            if !self.used[i as usize] {
                self.used[i as usize] = true;
                self.vals[i as usize] = 0;
                return Ok(i);
            }
            i += 1;
        }
        Err(TLS_NOFREE)
    }

    /// 置值：越界 → RANGE；未分配槽 → UNSET。
    pub fn set(&mut self, idx: u32, v: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(TLS_FROZEN);
        }
        if idx >= TLS_SLOTS {
            return Err(TLS_RANGE);
        }
        if !self.used[idx as usize] {
            return Err(TLS_UNSET);
        }
        self.vals[idx as usize] = v;
        Ok(())
    }

    /// 取值：越界 → RANGE；未分配槽 → UNSET。
    pub fn get(&self, idx: u32) -> Result<u64, u32> {
        if idx >= TLS_SLOTS {
            return Err(TLS_RANGE);
        }
        if !self.used[idx as usize] {
            return Err(TLS_UNSET);
        }
        Ok(self.vals[idx as usize])
    }

    /// 释放：越界 → RANGE；未分配（双重释放）→ DUPFREE。
    pub fn free(&mut self, idx: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TLS_FROZEN);
        }
        if idx >= TLS_SLOTS {
            return Err(TLS_RANGE);
        }
        if !self.used[idx as usize] {
            return Err(TLS_DUPFREE);
        }
        self.used[idx as usize] = false;
        self.vals[idx as usize] = 0;
        Ok(())
    }

    /// 在用槽计数。
    pub fn used_count(&self) -> u32 {
        let mut n = 0;
        for u in &self.used {
            if *u {
                n += 1;
            }
        }
        n
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for i in 0..TLS_SLOTS as usize {
            h = h.wrapping_mul(211).wrapping_add(self.used[i] as u32);
            h = h.wrapping_mul(211).wrapping_add(self.vals[i] as u32);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = TlsTable::new();
    // 1) 槽索引越界 → TLS_RANGE。
    got[0] = t.set(64, 1).err() == Some(TLS_RANGE) && t.get(99).err() == Some(TLS_RANGE);
    // 2) 读未分配槽 → TLS_UNSET。
    got[1] = t.get(0).err() == Some(TLS_UNSET);
    // 3) 槽满 → TLS_NOFREE。
    let mut full = TlsTable::new();
    let mut n = 0;
    while n < TLS_SLOTS {
        let _ = full.alloc();
        n += 1;
    }
    got[2] = full.alloc().err() == Some(TLS_NOFREE);
    // 4) 双重释放 → TLS_DUPFREE。
    let _ = t.alloc();
    let _ = t.free(0);
    got[3] = t.free(0).err() == Some(TLS_DUPFREE);
    // 5) 印账后分配 → TLS_FROZEN。
    t.seal();
    got[4] = t.alloc().err() == Some(TLS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12065_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12065");

    // J1 分配最小空闲槽。
    let j1 = {
        let mut t = TlsTable::new();
        t.alloc() == Ok(0) && t.alloc() == Ok(1)
    };
    s.add("J1-最小分配", j1, "0/1 递进");

    // J2 置取值往返。
    let j2 = {
        let mut t = TlsTable::new();
        let _ = t.alloc();
        let _ = t.set(0, 0xCAFE);
        t.get(0) == Ok(0xCAFE)
    };
    s.add("J2-置取往返", j2, "槽 0 值一致");

    // J3 未分配槽拒。
    let j3 = {
        let t = TlsTable::new();
        t.get(5).err() == Some(TLS_UNSET) && {
            let mut w = TlsTable::new();
            w.set(5, 1).err() == Some(TLS_UNSET)
        }
    };
    s.add("J3-未分配拒", j3, "读写两拒");

    // J4 越界拒。
    let j4 = {
        let mut t = TlsTable::new();
        let _ = t.alloc();
        t.get(64).err() == Some(TLS_RANGE) && t.set(64, 1).err() == Some(TLS_RANGE)
            && t.free(64).err() == Some(TLS_RANGE)
    };
    s.add("J4-越界拒", j4, "三面全拒");

    // J5 槽满。
    let j5 = {
        let mut t = TlsTable::new();
        let mut n = 0;
        while n < TLS_SLOTS {
            let _ = t.alloc();
            n += 1;
        }
        t.alloc().err() == Some(TLS_NOFREE) && t.used_count() == TLS_SLOTS
    };
    s.add("J5-槽满", j5, "64 槽全占用");

    // J6 释放复用。
    let j6 = {
        let mut t = TlsTable::new();
        let _ = t.alloc();
        let _ = t.alloc();
        let _ = t.free(0);
        t.alloc() == Ok(0)
    };
    s.add("J6-释放复用", j6, "槽 0 复用");

    // J7 双重释放拒。
    let j7 = {
        let mut t = TlsTable::new();
        let _ = t.alloc();
        let _ = t.free(0);
        t.free(0).err() == Some(TLS_DUPFREE)
    };
    s.add("J7-双释拒", j7, "空槽再放拒");

    // J8 槽间独立。
    let j8 = {
        let mut t = TlsTable::new();
        let _ = t.alloc();
        let _ = t.alloc();
        let _ = t.set(0, 0x11);
        let _ = t.set(1, 0x22);
        t.get(0) == Ok(0x11) && t.get(1) == Ok(0x22)
    };
    s.add("J8-槽独立", j8, "双值互扰");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = TlsTable::new();
        let _ = a.alloc();
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = TlsTable::new();
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异表异纹");

    // J10 印账冻结。
    let j10 = {
        let mut t = TlsTable::new();
        t.seal();
        t.alloc().err() == Some(TLS_FROZEN) && t.set(0, 1).err() == Some(TLS_FROZEN)
    };
    s.add("J10-印账冻结", j10, "印后动作拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例。
    let mut t = TlsTable::new();
    let _ = t.alloc();
    let _ = t.set(0, 0x5A5A);
    s.add("样例槽值", t.get(0) == Ok(0x5A5A) && t.used_count() == 1, "单槽 0x5A5A");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12065_alloc_cycle() {
        let mut t = TlsTable::new();
        assert_eq!(t.alloc(), Ok(0));
        assert_eq!(t.alloc(), Ok(1));
        assert!(t.free(0).is_ok());
        assert_eq!(t.alloc(), Ok(0));
    }

    #[test]
    fn unx12065_checks_green() {
        let s = run_unx12065_checks();
        assert!(s.all_passed(), "UNX-F12065 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12065_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
