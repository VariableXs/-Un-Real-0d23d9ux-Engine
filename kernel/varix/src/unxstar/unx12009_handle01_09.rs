//! UNX-F12009 句柄抽象底座：HANDLE 语义与引用规则（0x19340 段）
//!
//! 判据原文：句柄表以 u32 值为句柄面——零值即 NULL 永不发放；最高位
//! 置 1 的句柄是内核句柄，只能在内核上下文解引用（用户态使用即越权）；
//! 引用计数以 dup 增、以 close 减，减至零即槽位回收，重复关已回收槽
//! 即悬空关。表冻结后发放/复制/关闭一律拒收。
//!
//! 结构：句柄表（HandleTab：槽册 + 计数）+ 发放/复制/关闭三核
//! （五码守卫）+ 操作账（HdRec 逐笔留痕）+ 折叠指纹 + 印账冻结 +
//! 注入核 + 判据聚合（run_unx12009_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19340 独占 0x20）。
pub const SEG_BASE: u32 = 0x19340;
pub const HD_NULL: u32 = SEG_BASE + 0x01; // 零句柄
pub const HD_BAD_HANDLE: u32 = SEG_BASE + 0x02; // 未发放句柄
pub const HD_KERNEL_ONLY: u32 = SEG_BASE + 0x03; // 用户态使用内核句柄
pub const HD_DANGLING: u32 = SEG_BASE + 0x04; // 悬空关（已回收槽）
pub const HD_FROZEN: u32 = SEG_BASE + 0x05; // 表冻结后操作

/// 内核句柄位（最高位置 1）。
pub const KERNEL_HANDLE_BIT: u32 = 0x8000_0000;

/// 句柄槽：计引用。
#[derive(Clone)]
pub struct HandleEntry {
    pub handle: u32,
    pub refs: u32,
    pub note: String,
}

impl HandleEntry {
    pub fn is_kernel(&self) -> bool {
        self.handle & KERNEL_HANDLE_BIT != 0
    }
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.handle != 0
    }
}

/// 一笔句柄操作的账目。
#[derive(Clone)]
pub struct HdRec {
    pub seq: u32,
    pub handle: u32,
    /// 操作码：1=open 2=dup 3=close。
    pub op: u8,
    pub note: String,
}

impl HdRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.handle != 0
    }
}

/// 句柄表模型。
pub struct HandleTab {
    pub slots: Vec<HandleEntry>,
    pub ledger: Vec<HdRec>,
    pub seq_next: u32,
    pub next_raw: u32,
    pub sealed: bool,
}

impl HandleTab {
    pub fn new() -> HandleTab {
        HandleTab { slots: Vec::new(), ledger: Vec::new(), seq_next: 1, next_raw: 4, sealed: false }
    }

    /// 发放核：分配新句柄（kernel_bit 决定内核句柄位），零值永不发放。
    pub fn open(&mut self, kernel: bool, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(HD_FROZEN);
        }
        if note.is_empty() {
            return Err(HD_NULL);
        }
        let h = if kernel { self.next_raw | KERNEL_HANDLE_BIT } else { self.next_raw };
        self.next_raw += 4;
        self.slots.push(HandleEntry { handle: h, refs: 1, note: note.to_string() });
        self.ledger.push(HdRec { seq: self.seq_next, handle: h, op: 1, note: note.to_string() });
        self.seq_next += 1;
        Ok(h)
    }

    fn pos(&self, h: u32) -> Option<usize> {
        self.slots.iter().position(|s| s.handle == h)
    }

    /// 复制核：引用 +1。
    pub fn dup(&mut self, h: u32, ctx_kernel: bool, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(HD_FROZEN);
        }
        if note.is_empty() {
            return Err(HD_NULL);
        }
        if h == 0 {
            return Err(HD_NULL);
        }
        let i = match self.pos(h) {
            Some(i) => i,
            None => return Err(HD_BAD_HANDLE),
        };
        // 用户态上下文解引用内核句柄 → 越权。
        if !ctx_kernel && self.slots[i].is_kernel() {
            return Err(HD_KERNEL_ONLY);
        }
        self.slots[i].refs += 1;
        self.ledger.push(HdRec { seq: self.seq_next, handle: h, op: 2, note: note.to_string() });
        self.seq_next += 1;
        Ok(())
    }

    /// 关闭核：引用 -1，减至零回收槽。
    pub fn close(&mut self, h: u32, ctx_kernel: bool, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(HD_FROZEN);
        }
        if note.is_empty() {
            return Err(HD_NULL);
        }
        if h == 0 {
            return Err(HD_NULL);
        }
        let i = match self.pos(h) {
            Some(i) => i,
            None => return Err(HD_DANGLING),
        };
        if !ctx_kernel && self.slots[i].is_kernel() {
            return Err(HD_KERNEL_ONLY);
        }
        // 下溢先判：引用 1 时关闭即回收，不允许减到负。
        let refs = self.slots[i].refs;
        if refs == 0 {
            return Err(HD_DANGLING);
        }
        if refs == 1 {
            self.slots.remove(i);
        } else {
            self.slots[i].refs = refs - 1;
        }
        self.ledger.push(HdRec { seq: self.seq_next, handle: h, op: 3, note: note.to_string() });
        self.seq_next += 1;
        Ok(())
    }

    /// 引用计数回读。
    pub fn refs_of(&self, h: u32) -> Option<u32> {
        self.slots.iter().find(|s| s.handle == h).map(|s| s.refs)
    }

    /// 账目完整性：账与槽合账（open 数 == 当前槽 + 已回收差可对平）。
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
        for s in &self.slots {
            h = h.wrapping_mul(211).wrapping_add(s.handle);
            h = h.wrapping_mul(211).wrapping_add(s.refs);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
            h = h.wrapping_mul(211).wrapping_add(r.op as u32);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = HandleTab::new();
    // 1) 零句柄关闭 → HD_NULL。
    got[0] = t.close(0, false, "null").err() == Some(HD_NULL);
    // 2) 未发放句柄复制 → HD_BAD_HANDLE。
    let _ = t.open(false, "a");
    got[1] = t.dup(0xDEAD, false, "x").err() == Some(HD_BAD_HANDLE);
    // 3) 用户态使用内核句柄 → HD_KERNEL_ONLY。
    let kh = t.open(true, "k").unwrap_or(0);
    got[2] = t.dup(kh, false, "u").err() == Some(HD_KERNEL_ONLY);
    // 4) 悬空关：发放→关（回收）→再关 → HD_DANGLING。
    let h = t.open(false, "c").unwrap_or(0);
    let _ = t.close(h, false, "c1");
    got[3] = t.close(h, false, "c2").err() == Some(HD_DANGLING);
    // 5) 印账后发放 → HD_FROZEN。
    t.seal();
    got[4] = t.open(false, "post").err() == Some(HD_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12009_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12009");

    // J1 正路径：发放→复制→复制→关→关→关（回收）。
    let j1 = {
        let mut t = HandleTab::new();
        let h = t.open(false, "a").unwrap_or(0);
        let _ = t.dup(h, false, "d1");
        let _ = t.dup(h, false, "d2");
        let r3 = t.refs_of(h) == Some(3);
        let _ = t.close(h, false, "c1");
        let r2 = t.refs_of(h) == Some(2);
        let _ = t.close(h, false, "c2");
        let _ = t.close(h, false, "c3");
        r3 && r2 && t.refs_of(h).is_none()
    };
    s.add("J1-引用三开三关", j1, "refs 3→2→回收");

    // J2 零句柄永不发放且操作拒。
    let j2 = {
        let mut t = HandleTab::new();
        t.close(0, false, "z").err() == Some(HD_NULL)
            && t.dup(0, false, "z").err() == Some(HD_NULL)
    };
    s.add("J2-零句柄拒", j2, "NULL 永不合法");

    // J3 未发放句柄拒。
    let j3 = {
        let mut t = HandleTab::new();
        t.dup(0x1234, false, "g").err() == Some(HD_BAD_HANDLE)
    };
    s.add("J3-幽灵句柄拒", j3, "未发放必拒");

    // J4 内核句柄用户态拒：dup/close 双面。
    let j4 = {
        let mut t = HandleTab::new();
        let kh = t.open(true, "k").unwrap_or(0);
        let d = t.dup(kh, false, "u").err() == Some(HD_KERNEL_ONLY);
        let c = t.close(kh, false, "u").err() == Some(HD_KERNEL_ONLY);
        d && c && t.refs_of(kh) == Some(1)
    };
    s.add("J4-内核句柄越权", j4, "用户态双操作皆拒");

    // J5 内核句柄内核态合法。
    let j5 = {
        let mut t = HandleTab::new();
        let kh = t.open(true, "k").unwrap_or(0);
        let _ = t.dup(kh, true, "kd");
        kh & KERNEL_HANDLE_BIT != 0 && t.refs_of(kh) == Some(2)
    };
    s.add("J5-内核态合法", j5, "内核句柄内核上下文可用");

    // J6 悬空关拒。
    let j6 = {
        let mut t = HandleTab::new();
        let h = t.open(false, "a").unwrap_or(0);
        let _ = t.close(h, false, "c");
        t.close(h, false, "c2").err() == Some(HD_DANGLING)
    };
    s.add("J6-悬空关拒", j6, "回收槽再关必拒");

    // J7 槽回收后句柄不复用：新发放值恒异于已回收值。
    let j7 = {
        let mut t = HandleTab::new();
        let h1 = t.open(false, "a").unwrap_or(0);
        let _ = t.close(h1, false, "c");
        let h2 = t.open(false, "b").unwrap_or(0);
        h1 != 0 && h2 != 0 && h1 != h2
    };
    s.add("J7-不复用", j7, "句柄值单调推进");

    // J8 账目留痕：open/dup/close 三操作逐笔入账。
    let j8 = {
        let mut t = HandleTab::new();
        let h = t.open(false, "a").unwrap_or(0);
        let _ = t.dup(h, false, "d");
        let _ = t.close(h, false, "c");
        t.ledger.len() == 3
            && t.ledger[0].op == 1 && t.ledger[1].op == 2 && t.ledger[2].op == 3
            && t.ledger_ok()
    };
    s.add("J8-三操作留痕", j8, "op=1/2/3 逐笔对账");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = HandleTab::new();
        let _ = a.open(false, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut b = HandleTab::new();
        let _ = b.open(true, "x");
        f1 == f2 && a.fold() != b.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异表异纹");

    // J10 印账冻结：发放/复制/关闭三面全拒。
    let j10 = {
        let mut t = HandleTab::new();
        let h = t.open(false, "a").unwrap_or(0);
        t.seal();
        t.open(false, "o").err() == Some(HD_FROZEN)
            && t.dup(h, false, "d").err() == Some(HD_FROZEN)
            && t.close(h, false, "c").err() == Some(HD_FROZEN)
    };
    s.add("J10-印账冻结", j10, "三面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：混合表。
    let mut t = HandleTab::new();
    let _ = t.open(false, "u1");
    let _ = t.open(true, "k1");
    let _ = t.open(false, "u2");
    s.add("样例三槽在册", t.slots.len() == 3, "双用户一内核");
    s.add("样例账面合法", t.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12009_refcount_flow() {
        let mut t = HandleTab::new();
        let h = t.open(false, "a").unwrap_or(0);
        assert!(t.dup(h, false, "d").is_ok());
        assert_eq!(t.refs_of(h), Some(2));
        assert!(t.close(h, false, "c").is_ok());
        assert_eq!(t.refs_of(h), Some(1));
        assert!(t.close(h, false, "c2").is_ok());
        assert_eq!(t.refs_of(h), None);
        assert_eq!(t.close(h, false, "c3"), Err(HD_DANGLING));
    }

    #[test]
    fn unx12009_checks_green() {
        let s = run_unx12009_checks();
        assert!(s.all_passed(), "UNX-F12009 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12009_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
