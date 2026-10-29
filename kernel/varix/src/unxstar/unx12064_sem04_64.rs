//! UNX-F12064 LastErrorValue 线程局部错误码（0x19A20 段）
//!
//! 判据原文：LastErrorValue 是 TEB 中的 32 位线程局部错误槽；
//! 语义规则：内核态写（通过系统服务出口）与用户态 Win32 读；
//! 错误码按 Win32 面分区（0=成功；0x1..0x100 系统区；其余为
//! 应用区）；写账逐笔留痕；同值重写拒；封存后拒写。
//!
//! 结构：错误槽（ErrSlot 双域写）+ 错误码分区核 + 四码守卫 +
//! 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12064_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19A20 独占 0x20）。
pub const SEG_BASE: u32 = 0x19A20;
pub const LE_BAD_REC: u32 = SEG_BASE + 0x01; // 空留痕/伪账
pub const LE_NOCHANGE: u32 = SEG_BASE + 0x02; // 同值重写
pub const LE_FROZEN: u32 = SEG_BASE + 0x03; // 印账后仍写
pub const LE_ZONE: u32 = SEG_BASE + 0x04; // 越区错误码
pub const LE_BAD_DOMAIN: u32 = SEG_BASE + 0x05; // 写者域未知

/// Win32 错误码分区边界。
pub const ZONE_OK: u32 = 0;
pub const ZONE_SYS_HI: u32 = 0x100;

/// 错误码分区判定。
pub fn zone_of(code: u32) -> u32 {
    if code == ZONE_OK {
        0
    } else if code <= ZONE_SYS_HI {
        1
    } else {
        2
    }
}

/// 写者域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeWriter {
    Kernel,
    User,
}

/// 一笔错误写账。
#[derive(Clone)]
pub struct LeRec {
    pub seq: u32,
    pub old: u32,
    pub new: u32,
    pub note: String,
}

impl LeRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && self.old != self.new
    }
}

/// 错误槽。
pub struct ErrSlot {
    pub last_err: u32,
    pub recs: Vec<LeRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ErrSlot {
    pub fn new() -> ErrSlot {
        ErrSlot { last_err: 0, recs: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 写核：分区合法（<= 0xFFFF 窗）→ 同值拒 → 留痕。
    pub fn set(&mut self, who: LeWriter, code: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(LE_FROZEN);
        }
        if note.is_empty() {
            return Err(LE_BAD_REC);
        }
        if who != LeWriter::Kernel && who != LeWriter::User {
            return Err(LE_BAD_DOMAIN);
        }
        // 分区守卫：仅 0..=0xFFFF 可入槽（高位越区）。
        if code > 0xFFFF {
            return Err(LE_ZONE);
        }
        if code == self.last_err {
            return Err(LE_NOCHANGE);
        }
        let rec = LeRec { seq: self.seq_next, old: self.last_err, new: code, note: note.to_string() };
        if !rec.ok() {
            return Err(LE_BAD_REC);
        }
        self.seq_next += 1;
        self.recs.push(rec);
        self.last_err = code;
        Ok(())
    }

    /// 读（用户态 Win32 面只读）。
    pub fn get(&self) -> u32 {
        self.last_err
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.recs.iter().all(|r| r.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.last_err);
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut e = ErrSlot::new();
    // 1) 空留痕 → LE_BAD_REC。
    got[0] = e.set(LeWriter::Kernel, 2, "").err() == Some(LE_BAD_REC);
    // 2) 越区错误码（>0xFFFF）→ LE_ZONE。
    got[1] = e.set(LeWriter::Kernel, 0x1_0000, "z").err() == Some(LE_ZONE);
    // 3) 同值重写 → LE_NOCHANGE。
    let _ = e.set(LeWriter::Kernel, 5, "ok");
    got[2] = e.set(LeWriter::User, 5, "dup").err() == Some(LE_NOCHANGE);
    // 4) 伪账（手工构造空痕账目）。
    let forged = LeRec { seq: 1, old: 1, new: 2, note: String::new() };
    got[3] = !forged.ok();
    // 5) 印账后写 → LE_FROZEN。
    e.seal();
    got[4] = e.set(LeWriter::Kernel, 9, "post").err() == Some(LE_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12064_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12064");

    // J1 初值零（无错误）。
    let j1 = ErrSlot::new().get() == 0;
    s.add("J1-零初值", j1, "未写回零");

    // J2 内核写 → 用户读。
    let j2 = {
        let mut e = ErrSlot::new();
        let _ = e.set(LeWriter::Kernel, 0x57, "param"); // ERROR_INVALID_PARAMETER
        e.get() == 0x57
    };
    s.add("J2-写读面", j2, "内核写用户读");

    // J3 分区三面。
    let j3 = zone_of(0) == 0 && zone_of(0x57) == 1 && zone_of(0x2001) == 2;
    s.add("J3-分区三面", j3, "0/系统/应用");

    // J4 同值重写拒（跨域同样拒）。
    let j4 = {
        let mut e = ErrSlot::new();
        let _ = e.set(LeWriter::Kernel, 3, "a");
        e.set(LeWriter::User, 3, "b").err() == Some(LE_NOCHANGE)
    };
    s.add("J4-同值拒", j4, "跨域空变更拒");

    // J5 越区拒。
    let j5 = {
        let mut e = ErrSlot::new();
        e.set(LeWriter::Kernel, 0x1_0000, "z").err() == Some(LE_ZONE)
            && e.set(LeWriter::User, 0xFFFF_FFFF, "z").err() == Some(LE_ZONE)
    };
    s.add("J5-越区拒", j5, "高位全拒");

    // J6 边界值可入槽（0xFFFF 系统/应用界内最大）。
    let j6 = {
        let mut e = ErrSlot::new();
        e.set(LeWriter::User, 0xFFFF, "edge").is_ok() && e.get() == 0xFFFF
    };
    s.add("J6-界内最大", j6, "0xFFFF 过");

    // J7 留痕对账。
    let j7 = {
        let mut e = ErrSlot::new();
        let _ = e.set(LeWriter::Kernel, 2, "r1");
        let _ = e.set(LeWriter::User, 0, "r2");
        e.recs.len() == 2 && e.recs[0].old == 0 && e.recs[0].new == 2
            && e.recs[1].old == 2 && e.recs[1].new == 0 && e.ledger_ok()
    };
    s.add("J7-留痕对账", j7, "新旧值链对账");

    // J8 清错路径（写 0）。
    let j8 = {
        let mut e = ErrSlot::new();
        let _ = e.set(LeWriter::Kernel, 0x5, "err");
        let _ = e.set(LeWriter::Kernel, 0, "clear");
        e.get() == 0
    };
    s.add("J8-清错", j8, "写零即清");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = ErrSlot::new();
        let _ = a.set(LeWriter::Kernel, 4, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = ErrSlot::new();
        let _ = c.set(LeWriter::Kernel, 6, "x");
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异槽异纹");

    // J10 印账冻结。
    let j10 = {
        let mut e = ErrSlot::new();
        e.seal();
        e.set(LeWriter::Kernel, 1, "post").err() == Some(LE_FROZEN)
    };
    s.add("J10-印账冻结", j10, "印后写拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例。
    let mut e = ErrSlot::new();
    let _ = e.set(LeWriter::Kernel, 0x2, "file-not-found");
    s.add("样例错误面", e.get() == 0x2 && e.ledger_ok(), "2 号留痕");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12064_write_read() {
        let mut e = ErrSlot::new();
        assert!(e.set(LeWriter::Kernel, 0x57, "a").is_ok());
        assert_eq!(e.get(), 0x57);
        assert_eq!(e.set(LeWriter::Kernel, 0x57, "b").err(), Some(LE_NOCHANGE));
    }

    #[test]
    fn unx12064_checks_green() {
        let s = run_unx12064_checks();
        assert!(s.all_passed(), "UNX-F12064 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12064_zone_ledger() {
        assert_eq!(zone_of(0), 0);
        assert_eq!(zone_of(1), 1);
        assert_eq!(zone_of(0x100), 1);
        assert_eq!(zone_of(0x101), 2);
        let mut e = ErrSlot::new();
        assert!(e.set(LeWriter::Kernel, 0x57, "a").is_ok());
        assert!(e.set(LeWriter::User, 0, "b").is_ok());
        assert_eq!(e.recs.len(), 2);
        assert_eq!(e.recs[0].old, 0);
        assert_eq!(e.recs[1].new, 0);
        assert!(e.ledger_ok());
        assert_ne!(e.fold(), SEG_BASE);
    }

    #[test]
    fn unx12064_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
