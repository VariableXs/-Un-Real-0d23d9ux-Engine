//! UNX-F12042 ImageBaseAddress 语义与 D2 装载交接挂点（0x19760 段）
//!
//! 判据原文：PEB.ImageBaseAddress 槽——基址必须 64KB 对齐（x64 映像
//! 对齐钉值）；装载交接单次性（首次定基，重定位须走显式重定位面且
//! 仅一次）；零基址拒；重定位后旧基留账可查。
//!
//! 结构：基址槽（BaseSlot：基址 × 重定位账）+ 定基/重定位双核（四码
//! 守卫）+ 账目（BsRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12042_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19760 独占 0x20）。
pub const SEG_BASE: u32 = 0x19760;
pub const BS_ZERO: u32 = SEG_BASE + 0x01; // 零基址
pub const BS_UNALIGNED: u32 = SEG_BASE + 0x02; // 非 64KB 对齐
pub const BS_DOUBLE_SET: u32 = SEG_BASE + 0x03; // 重复定基
pub const BS_RELOC_TWICE: u32 = SEG_BASE + 0x04; // 双重重定位
pub const BS_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// x64 映像 64KB 对齐钉值。
pub const IMAGE_ALIGN: u64 = 0x1_0000;

/// 一笔基址动作的账目。
#[derive(Clone)]
pub struct BsRec {
    pub seq: u32,
    /// 操作码：1=定基 2=重定位。
    pub op: u8,
    pub base: u64,
    pub note: String,
}

impl BsRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && self.base != 0
    }
}

/// 基址槽模型。
pub struct BaseSlot {
    pub base: Option<u64>,
    pub reloc_used: bool,
    pub ledger: Vec<BsRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl BaseSlot {
    pub fn new() -> BaseSlot {
        BaseSlot { base: None, reloc_used: false, ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    fn aligned(v: u64) -> bool {
        v != 0 && v & (IMAGE_ALIGN - 1) == 0
    }

    /// 定基核：对齐 + 单次。
    pub fn set_base(&mut self, base: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(BS_FROZEN);
        }
        if note.is_empty() {
            return Err(BS_ZERO);
        }
        if base == 0 {
            return Err(BS_ZERO);
        }
        if !Self::aligned(base) {
            return Err(BS_UNALIGNED);
        }
        if self.base.is_some() {
            return Err(BS_DOUBLE_SET);
        }
        let rec = BsRec { seq: self.seq_next, op: 1, base, note: note.to_string() };
        if !rec.ok() {
            return Err(BS_ZERO);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        self.base = Some(base);
        Ok(())
    }

    /// 重定位核：单次，改基留旧账。
    pub fn relocate(&mut self, new_base: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(BS_FROZEN);
        }
        if note.is_empty() {
            return Err(BS_ZERO);
        }
        if !Self::aligned(new_base) {
            return Err(BS_UNALIGNED);
        }
        if self.base.is_none() {
            return Err(BS_DOUBLE_SET);
        }
        if self.reloc_used {
            return Err(BS_RELOC_TWICE);
        }
        let rec = BsRec { seq: self.seq_next, op: 2, base: new_base, note: note.to_string() };
        if !rec.ok() {
            return Err(BS_ZERO);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        self.reloc_used = true;
        self.base = Some(new_base);
        Ok(())
    }

    /// 当前基址。
    pub fn current(&self) -> Option<u64> {
        self.base
    }

    /// 旧基回读（重定位前的基址）。
    pub fn original_base(&self) -> Option<u64> {
        self.ledger.iter().find(|r| r.op == 1).map(|r| r.base)
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.base.unwrap_or(0) as u32);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = BaseSlot::new();
    // 1) 零基址 → BS_ZERO。
    got[0] = b.set_base(0, "z").err() == Some(BS_ZERO);
    // 2) 非 64KB 对齐 → BS_UNALIGNED。
    got[1] = b.set_base(0x1_4001_0001, "u").err() == Some(BS_UNALIGNED);
    // 3) 重复定基 → BS_DOUBLE_SET。
    let _ = b.set_base(0x0040_0000, "ok");
    got[2] = b.set_base(0x0041_0000, "dup").err() == Some(BS_DOUBLE_SET);
    // 4) 双重重定位 → BS_RELOC_TWICE。
    let _ = b.relocate(0x0042_0000, "r1");
    got[3] = b.relocate(0x0043_0000, "r2").err() == Some(BS_RELOC_TWICE);
    // 5) 印账后动作 → BS_FROZEN。
    b.seal();
    got[4] = b.relocate(0x0044_0000, "post").err() == Some(BS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12042_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12042");

    // J1 正路径：定基→重定位→基址与旧基可查。
    let j1 = {
        let mut b = BaseSlot::new();
        let _ = b.set_base(0x0040_0000, "load");
        let _ = b.relocate(0x7FF6_0000, "aslr");
        b.current() == Some(0x7FF6_0000) && b.original_base() == Some(0x0040_0000)
    };
    s.add("J1-定基重定位", j1, "旧新双基对账");

    // J2 零基址拒。
    let j2 = { BaseSlot::new().set_base(0, "z").err() == Some(BS_ZERO) };
    s.add("J2-零基址拒", j2, "零永不合法");

    // J3 对齐面：差一字节拒、恰对齐收。
    let j3 = {
        let mut a = BaseSlot::new();
        let bad = a.set_base(0x0040_0001, "u").err() == Some(BS_UNALIGNED);
        let mut c = BaseSlot::new();
        let good = c.set_base(0x0040_0000, "a").is_ok();
        bad && good
    };
    s.add("J3-64K 对齐", j3, "0x10000 对齐约束");

    // J4 重复定基拒。
    let j4 = {
        let mut b = BaseSlot::new();
        let _ = b.set_base(0x0040_0000, "a");
        b.set_base(0x0041_0000, "b").err() == Some(BS_DOUBLE_SET)
    };
    s.add("J4-重复定基拒", j4, "定基单次性");

    // J5 双重重定位拒。
    let j5 = {
        let mut b = BaseSlot::new();
        let _ = b.set_base(0x0040_0000, "a");
        let _ = b.relocate(0x0041_0000, "r");
        b.relocate(0x0042_0000, "r2").err() == Some(BS_RELOC_TWICE)
    };
    s.add("J5-双重定位拒", j5, "重定位单次性");

    // J6 未定基先重定位拒。
    let j6 = {
        let mut b = BaseSlot::new();
        b.relocate(0x0040_0000, "r").err() == Some(BS_DOUBLE_SET)
    };
    s.add("J6-未定基拒", j6, "先定基后重定位");

    // J7 对齐钉值核对。
    let j7 = IMAGE_ALIGN == 0x1_0000;
    s.add("J7-对齐钉值", j7, "64KB");

    // J8 账目留痕：op=1/2。
    let j8 = {
        let mut b = BaseSlot::new();
        let _ = b.set_base(0x0040_0000, "a");
        let _ = b.relocate(0x0041_0000, "b");
        b.ledger.len() == 2 && b.ledger[0].op == 1 && b.ledger[1].op == 2 && b.ledger_ok()
    };
    s.add("J8-双操作留痕", j8, "op=1/2 对账");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = BaseSlot::new();
        let _ = a.set_base(0x0040_0000, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = BaseSlot::new();
        let _ = c.set_base(0x0041_0000, "x");
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异槽异纹");

    // J10 印账冻结。
    let j10 = {
        let mut b = BaseSlot::new();
        let _ = b.set_base(0x0040_0000, "a");
        b.seal();
        b.set_base(0x0041_0000, "post").err() == Some(BS_FROZEN)
            && b.relocate(0x0041_0000, "post").err() == Some(BS_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：典型 ASLR 面。
    let mut b = BaseSlot::new();
    let _ = b.set_base(0x0000_0040_0000, "sum-pre");
    let _ = b.relocate(0x0000_7FF6_4000, "sum-aslr");
    s.add("样例双基", b.original_base() == Some(0x40_0000), "偏好基在账");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12042_base_flow() {
        let mut b = BaseSlot::new();
        assert!(b.set_base(0x4000_0000, "a").is_ok());
        assert_eq!(b.set_base(0x4001_0000, "b"), Err(BS_DOUBLE_SET));
        assert!(b.relocate(0x7FF6_0000, "c").is_ok());
        assert_eq!(b.relocate(0x7FF7_0000, "d"), Err(BS_RELOC_TWICE));
        assert_eq!(b.current(), Some(0x7FF6_0000));
    }

    #[test]
    fn unx12042_checks_green() {
        let s = run_unx12042_checks();
        assert!(s.all_passed(), "UNX-F12042 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12042_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
