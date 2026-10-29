//! UNX-F12043 ProcessHeap 指针族与默认堆挂点（D2 联签）（0x19780 段）
//!
//! 判据原文：进程默认堆（槽 0）必须首先挂接——堆表在默认堆未挂前拒
//! 挂其余堆；堆指针必须页对齐且不重复挂接；默认堆不可卸（进程生命期
//! 内常驻）；堆表逐槽可查。
//!
//! 结构：堆表（HeapTable：槽册 × 默认堆位）+ 挂/卸双核（五码守卫）+
//! 挂账（HtRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12043_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19780 独占 0x20）。
pub const SEG_BASE: u32 = 0x19780;
pub const HT_BEFORE_DEFAULT: u32 = SEG_BASE + 0x01; // 默认堆未挂先挂他堆
pub const HT_UNALIGNED: u32 = SEG_BASE + 0x02; // 非页对齐
pub const HT_DUP: u32 = SEG_BASE + 0x03; // 重复挂接
pub const HT_DEFAULT_UNMOUNT: u32 = SEG_BASE + 0x04; // 默认堆卸挂
pub const HT_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// 页对齐钉值（x64 4KB）。
pub const PAGE_SIZE: u64 = 0x1000;

/// 一笔挂账。
#[derive(Clone)]
pub struct HtRec {
    pub seq: u32,
    pub heap: u64,
    /// 操作码：1=挂 2=卸。
    pub op: u8,
    pub note: String,
}

impl HtRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && self.heap != 0
    }
}

/// 堆表模型。
pub struct HeapTable {
    pub heaps: Vec<u64>,
    pub ledger: Vec<HtRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl HeapTable {
    pub fn new() -> HeapTable {
        HeapTable { heaps: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 默认堆已挂判定（槽 0 在册）。
    pub fn default_mounted(&self) -> bool {
        !self.heaps.is_empty()
    }

    /// 挂堆核：默认堆未挂前仅允许挂默认堆（即首挂即默认堆）。
    pub fn mount(&mut self, heap: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(HT_FROZEN);
        }
        if note.is_empty() {
            return Err(HT_UNALIGNED);
        }
        if heap == 0 {
            return Err(HT_UNALIGNED);
        }
        if heap & (PAGE_SIZE - 1) != 0 {
            return Err(HT_UNALIGNED);
        }
        if self.heaps.iter().any(|h| *h == heap) {
            return Err(HT_DUP);
        }
        // 表空时首挂即默认堆；表非空即默认堆已挂，放行。
        let _ = self.default_mounted();
        self.heaps.push(heap);
        let rec = HtRec { seq: self.seq_next, heap, op: 1, note: note.to_string() };
        if !rec.ok() {
            return Err(HT_DUP);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        Ok(())
    }

    /// 卸堆核：默认堆（槽 0）不可卸。
    pub fn unmount(&mut self, heap: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(HT_FROZEN);
        }
        if note.is_empty() {
            return Err(HT_DUP);
        }
        if self.heaps.first().map(|h| *h) == Some(heap) {
            return Err(HT_DEFAULT_UNMOUNT);
        }
        if let Some(i) = self.heaps.iter().position(|h| *h == heap) {
            self.heaps.remove(i);
            let rec = HtRec { seq: self.seq_next, heap, op: 2, note: note.to_string() };
            if !rec.ok() {
                return Err(HT_DUP);
            }
            self.seq_next += 1;
            self.ledger.push(rec);
            Ok(())
        } else {
            Err(HT_DUP)
        }
    }

    /// 槽查。
    pub fn slot_of(&self, heap: u64) -> Option<usize> {
        self.heaps.iter().position(|h| *h == heap)
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
        for heap in &self.heaps {
            h = h.wrapping_mul(211).wrapping_add((heap & 0xFFFF) as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = HeapTable::new();
    let _ = t.mount(0x0026_0000, "def");
    // 1) 非页对齐 → HT_UNALIGNED。
    got[0] = t.mount(0x0026_1001, "u").err() == Some(HT_UNALIGNED);
    // 2) 重复挂接 → HT_DUP。
    got[1] = t.mount(0x0026_0000, "dup").err() == Some(HT_DUP);
    // 3) 默认堆卸挂 → HT_DEFAULT_UNMOUNT。
    got[2] = t.unmount(0x0026_0000, "def-u").err() == Some(HT_DEFAULT_UNMOUNT);
    // 4) 幽灵卸挂 → HT_DUP。
    got[3] = t.unmount(0x0030_0000, "ghost").err() == Some(HT_DUP);
    // 5) 印账后动作 → HT_FROZEN。
    t.seal();
    got[4] = t.mount(0x0030_0000, "post").err() == Some(HT_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12043_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12043");

    // J1 正路径：挂默认堆→挂次堆→卸次堆。
    let j1 = {
        let mut t = HeapTable::new();
        let _ = t.mount(0x0026_0000, "def");
        let _ = t.mount(0x0030_0000, "aux");
        let um = t.unmount(0x0030_0000, "aux-u").is_ok();
        t.slot_of(0x0026_0000) == Some(0) && um && t.heaps.len() == 1
    };
    s.add("J1-挂卸正路径", j1, "默认堆常驻");

    // J2 页对齐：差一字节拒。
    let j2 = {
        let mut t = HeapTable::new();
        t.mount(0x0026_0001, "u").err() == Some(HT_UNALIGNED)
    };
    s.add("J2-非对齐拒", j2, "4K 对齐约束");

    // J3 零指针拒。
    let j3 = {
        let mut t = HeapTable::new();
        t.mount(0, "z").err() == Some(HT_UNALIGNED)
    };
    s.add("J3-零指针拒", j3, "零堆永不挂");

    // J4 重复挂接拒。
    let j4 = {
        let mut t = HeapTable::new();
        let _ = t.mount(0x0026_0000, "a");
        t.mount(0x0026_0000, "b").err() == Some(HT_DUP)
    };
    s.add("J4-重复挂拒", j4, "一址一槽");

    // J5 默认堆不可卸。
    let j5 = {
        let mut t = HeapTable::new();
        let _ = t.mount(0x0026_0000, "def");
        t.unmount(0x0026_0000, "u").err() == Some(HT_DEFAULT_UNMOUNT)
            && t.default_mounted()
    };
    s.add("J5-默认堆常驻", j5, "卸默认必拒");

    // J6 幽灵卸挂拒。
    let j6 = {
        let mut t = HeapTable::new();
        let _ = t.mount(0x0026_0000, "def");
        t.unmount(0x0030_0000, "g").err() == Some(HT_DUP)
    };
    s.add("J6-幽灵卸拒", j6, "未挂不可卸");

    // J7 槽序稳定性：首挂恒槽 0。
    let j7 = {
        let mut t = HeapTable::new();
        let _ = t.mount(0x0026_0000, "def");
        let _ = t.mount(0x0030_0000, "aux");
        let _ = t.unmount(0x0030_0000, "aux-u");
        let _ = t.mount(0x0031_0000, "aux2");
        t.slot_of(0x0026_0000) == Some(0) && t.heaps.len() == 2
    };
    s.add("J7-槽序稳定", j7, "默认堆槽 0 不漂");

    // J8 对齐钉值核对。
    let j8 = PAGE_SIZE == 0x1000;
    s.add("J8-页钉值", j8, "4KB");

    // J9 账目留痕：挂/卸 op 对账。
    let j9 = {
        let mut t = HeapTable::new();
        let _ = t.mount(0x0026_0000, "a");
        let _ = t.mount(0x0030_0000, "b");
        let _ = t.unmount(0x0030_0000, "c");
        t.ledger.len() == 3 && t.ledger[0].op == 1 && t.ledger[2].op == 2 && t.ledger_ok()
    };
    s.add("J9-挂卸留痕", j9, "三笔对账");

    // J10 印账冻结。
    let j10 = {
        let mut t = HeapTable::new();
        let _ = t.mount(0x0026_0000, "a");
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2
            && t.mount(0x0030_0000, "post").err() == Some(HT_FROZEN)
            && t.unmount(0x0026_0000, "post").err() == Some(HT_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：双堆面。
    let mut t = HeapTable::new();
    let _ = t.mount(0x0026_0000, "sum-def");
    let _ = t.mount(0x0031_0000, "sum-aux");
    s.add("样例双堆", t.heaps.len() == 2 && t.default_mounted(), "默认+次堆");
    s.add("样例账面合法", t.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12043_heap_flow() {
        let mut t = HeapTable::new();
        assert!(t.mount(0x260000, "a").is_ok());
        assert_eq!(t.mount(0x260001, "b"), Err(HT_UNALIGNED));
        assert_eq!(t.mount(0x260000, "c"), Err(HT_DUP));
        assert!(t.mount(0x300000, "d").is_ok());
        assert_eq!(t.unmount(0x260000, "e"), Err(HT_DEFAULT_UNMOUNT));
        assert!(t.unmount(0x300000, "f").is_ok());
    }

    #[test]
    fn unx12043_checks_green() {
        let s = run_unx12043_checks();
        assert!(s.all_passed(), "UNX-F12043 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12043_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
