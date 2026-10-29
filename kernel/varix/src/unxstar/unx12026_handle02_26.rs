//! UNX-F12026 STATUS_INVALID_HANDLE 句柄错误族与句柄账联签（0x19560 段）
//!
//! 判据原文：句柄校验三拒面——零句柄（NULL）、内核句柄入用户态解引、
//! 表中无此句柄，各踩 STATUS_INVALID_HANDLE（0xC0000008）同一错误族但
//! 留痕分面；拒面逐笔入红账并可对账（三拒面清点合计==红账总数）。
//!
//! 结构：句柄校验器（HandleGate：在册句柄集 + 内核位标记）+ 校验核
//! （四码守卫）+ 红账（HgRec 分面留痕）+ 折叠指纹 + 印账冻结 + 注入核 +
//! 判据聚合（run_unx12026_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19560 独占 0x20）。
pub const SEG_BASE: u32 = 0x19560;
pub const HG_NULL: u32 = SEG_BASE + 0x01; // 零句柄
pub const HG_KERNEL_IN_USER: u32 = SEG_BASE + 0x02; // 内核句柄入用户态
pub const HG_UNKNOWN: u32 = SEG_BASE + 0x03; // 表中无此句柄
pub const HG_BAD_REC: u32 = SEG_BASE + 0x04; // 伪造红账
pub const HG_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍校验

/// STATUS_INVALID_HANDLE 语义钉值。
pub const STATUS_INVALID_HANDLE: u32 = 0xC000_0008;

/// 内核句柄位。
pub const KH_BIT: u32 = 0x8000_0000;

/// 拒面分类。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HgFace {
    Null = 1,
    KernelInUser = 2,
    Unknown = 3,
}

/// 一笔红账。
#[derive(Clone)]
pub struct HgRec {
    pub seq: u32,
    pub handle: u32,
    pub face: HgFace,
    pub note: String,
}

impl HgRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 句柄校验器。
pub struct HandleGate {
    /// 在册句柄（含内核位标记）。
    pub table: Vec<(u32, bool)>,
    pub ledger: Vec<HgRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl HandleGate {
    pub fn new() -> HandleGate {
        HandleGate { table: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 发放在册句柄。
    pub fn enlist(&mut self, h: u32, kernel: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(HG_FROZEN);
        }
        if h == 0 {
            return Err(HG_NULL);
        }
        if self.table.iter().any(|t| t.0 == h) {
            return Err(HG_UNKNOWN);
        }
        self.table.push((h, kernel));
        Ok(())
    }

    fn enlisted(&self, h: u32) -> Option<bool> {
        self.table.iter().find(|t| t.0 == h).map(|t| t.1)
    }

    /// 校验核：三拒面 + 放行。
    pub fn validate(&mut self, h: u32, ctx_kernel: bool, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(HG_FROZEN);
        }
        if note.is_empty() {
            return Err(HG_BAD_REC);
        }
        let face = if h == 0 {
            HgFace::Null
        } else if !ctx_kernel && h & KH_BIT != 0 {
            HgFace::KernelInUser
        } else if self.enlisted(h).is_none() {
            HgFace::Unknown
        } else {
            return Ok(());
        };
        let rec = HgRec { seq: self.seq_next, handle: h, face, note: note.to_string() };
        if !rec.ok() {
            return Err(HG_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        Err(HG_NULL)
    }

    /// 拒面清点。
    pub fn count_face(&self, f: HgFace) -> usize {
        self.ledger.iter().filter(|r| r.face == f).count()
    }

    /// 对平面：三拒面合计==红账总数。
    pub fn faces_tally(&self) -> bool {
        self.count_face(HgFace::Null)
            + self.count_face(HgFace::KernelInUser)
            + self.count_face(HgFace::Unknown)
            == self.ledger.len()
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
        for t in &self.table {
            h = h.wrapping_mul(211).wrapping_add(t.0);
            h = h.wrapping_mul(211).wrapping_add(t.1 as u32);
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
    let mut g = HandleGate::new();
    let _ = g.enlist(0x10, false);
    let _ = g.enlist(0x8000_0020, true);
    // 1) 零句柄 → HG_NULL 拒面。
    got[0] = g.validate(0, false, "z").is_err();
    // 2) 内核句柄入用户态 → HG_KERNEL_IN_USER 拒面。
    got[1] = g.validate(0x8000_0020, false, "k").is_err();
    // 3) 表中无此句柄 → HG_UNKNOWN 拒面。
    got[2] = g.validate(0xDEAD, false, "u").is_err();
    // 4) 空留痕 → HG_BAD_REC。
    got[3] = g.validate(0x10, false, "").err() == Some(HG_BAD_REC);
    // 5) 印账后校验 → HG_FROZEN。
    g.seal();
    got[4] = g.validate(0x10, false, "post").err() == Some(HG_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12026_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12026");

    // J1 三拒面各踩一脚：清点 1/1/1。
    let j1 = {
        let mut g = HandleGate::new();
        let _ = g.enlist(0x10, false);
        let _ = g.enlist(0x8000_0020, true);
        let _ = g.validate(0, false, "z");
        let _ = g.validate(0x8000_0020, false, "k");
        let _ = g.validate(0xDEAD, false, "u");
        g.count_face(HgFace::Null) == 1
            && g.count_face(HgFace::KernelInUser) == 1
            && g.count_face(HgFace::Unknown) == 1
            && g.faces_tally()
    };
    s.add("J1-三拒面清点", j1, "各一脚对平");

    // J2 在册句柄放行：用户句柄用户态、内核句柄内核态。
    let j2 = {
        let mut g = HandleGate::new();
        let _ = g.enlist(0x10, false);
        let _ = g.enlist(0x8000_0020, true);
        g.validate(0x10, false, "u").is_ok() && g.validate(0x8000_0020, true, "k").is_ok()
    };
    s.add("J2-在册放行", j2, "双面上下文全通");

    // J3 内核句柄内核态放行、用户态拒。
    let j3 = {
        let mut g = HandleGate::new();
        let _ = g.enlist(0x8000_0040, true);
        let ok = g.validate(0x8000_0040, true, "k").is_ok();
        let bad = g.validate(0x8000_0040, false, "u").is_err();
        ok && bad
    };
    s.add("J3-内核位分界", j3, "上下文分界精确");

    // J4 零句柄永不发放。
    let j4 = {
        let mut g = HandleGate::new();
        g.enlist(0, false) == Err(HG_NULL)
    };
    s.add("J4-零号不发放", j4, "NULL 永不在册");

    // J5 重复发放拒。
    let j5 = {
        let mut g = HandleGate::new();
        let _ = g.enlist(0x20, false);
        g.enlist(0x20, false) == Err(HG_UNKNOWN)
    };
    s.add("J5-重发放拒", j5, "句柄唯一在册");

    // J6 红账留痕：handle 与 face 对账。
    let j6 = {
        let mut g = HandleGate::new();
        let _ = g.validate(0xDEAD, false, "u");
        g.ledger[0].handle == 0xDEAD && g.ledger[0].face == HgFace::Unknown && g.ledger_ok()
    };
    s.add("J6-红账对账", j6, "面与句柄同记");

    // J7 空留痕拒。
    let j7 = {
        let mut g = HandleGate::new();
        g.validate(0x10, false, "").err() == Some(HG_BAD_REC)
    };
    s.add("J7-空留痕拒", j7, "留痕必填");

    // J8 指纹区分度与幂等。
    let j8 = {
        let mut a = HandleGate::new();
        let _ = a.enlist(0x10, false);
        let f1 = a.fold();
        let f2 = a.fold();
        let mut b = HandleGate::new();
        let _ = b.enlist(0x10, true);
        f1 == f2 && a.fold() != b.fold()
    };
    s.add("J8-指纹性质", j8, "幂等且异表异纹");

    // J9 印账冻结。
    let j9 = {
        let mut g = HandleGate::new();
        let _ = g.enlist(0x10, false);
        g.seal();
        g.validate(0x10, false, "post").err() == Some(HG_FROZEN)
            && g.enlist(0x20, false).err() == Some(HG_FROZEN)
    };
    s.add("J9-印账冻结", j9, "双面操作印后全拒");

    // J10 拒面精确性：三面枚举值核对。
    let j10 = HgFace::Null as u16 == 1
        && HgFace::KernelInUser as u16 == 2
        && HgFace::Unknown as u16 == 3;
    s.add("J10-拒面枚举", j10, "1/2/3 钉值");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：混合表三面红账。
    let mut g = HandleGate::new();
    let _ = g.enlist(0x30, false);
    let _ = g.validate(0, false, "sum-z");
    let _ = g.validate(0x8000_0050, false, "sum-k");
    let _ = g.validate(0x1111, false, "sum-u");
    let _ = g.validate(0x30, false, "sum-ok");
    s.add("样例三拒一放", g.ledger.len() == 3 && g.faces_tally(), "三面红账对平");
    s.add("样例账面合法", g.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12026_gate_flow() {
        let mut g = HandleGate::new();
        assert!(g.enlist(0x10, false).is_ok());
        assert!(g.enlist(0x8000_0020, true).is_ok());
        assert!(g.validate(0x10, false, "u").is_ok());
        assert!(g.validate(0x8000_0020, true, "k").is_ok());
        let _ = g.validate(0x8000_0020, false, "bad");
        assert_eq!(g.count_face(HgFace::KernelInUser), 1);
    }

    #[test]
    fn unx12026_checks_green() {
        let s = run_unx12026_checks();
        assert!(s.all_passed(), "UNX-F12026 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12026_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
