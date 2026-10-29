//! UNX-F12038 客户码位与保留区间账（Facility 全表）（0x196E0 段）
//!
//! 判据原文：设施全表账——设施号 12 位域切为官方段（0x0..0x0FF 钉定
//! 已知设施）与保留段（0x100..0x7FF）；客户位（位 29）置位后设施号可
//! 入客户段（0x800..0xFFF）；官方设施登记须与钉定名对表；保留段直登
//! 即拒（须走客户位）。
//!
//! 结构：设施全表（FacTable：设施号 × 段类 × 名）+ 登记核（四码守卫）+
//! 段账（FaRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12038_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x196E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x196E0;
pub const FA_RESERVED_PLAIN: u32 = SEG_BASE + 0x01; // 保留段未走客户位
pub const FA_DUP: u32 = SEG_BASE + 0x02; // 设施重复登记
pub const FA_BAD_NAME: u32 = SEG_BASE + 0x03; // 空名登记
pub const FA_UNKNOWN_OFFICIAL: u32 = SEG_BASE + 0x04; // 官方段不在钉定名表
pub const FA_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍登记

/// 官方段上界（0x0..0xFF）。
pub const OFFICIAL_MAX: u32 = 0x0FF;
/// 保留段下界/上界（0x100..0x7FF）。
pub const RESERVED_LO: u32 = 0x100;
pub const RESERVED_HI: u32 = 0x7FF;
/// 客户段下界（0x800..0xFFF）。
pub const CUSTOMER_LO: u32 = 0x800;

/// 段类闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FacSeg {
    Official = 1,
    Reserved = 2,
    Customer = 3,
}

/// 段类判定：设施号 + 客户位。
pub fn seg_of(fac: u32, customer_bit: u32) -> Option<FacSeg> {
    if fac > 0xFFF {
        return None;
    }
    if fac <= OFFICIAL_MAX {
        Some(FacSeg::Official)
    } else if fac >= RESERVED_LO && fac <= RESERVED_HI {
        if customer_bit == 1 {
            Some(FacSeg::Customer)
        } else {
            Some(FacSeg::Reserved)
        }
    } else {
        Some(FacSeg::Customer)
    }
}

/// 官方设施钉定名表（精选）。
pub fn official_name(fac: u32) -> Option<&'static str> {
    match fac {
        0x0 => Some("NTCORE"),
        0xB => Some("NTDEBUG"),
        0x5 => Some("NTKERNEL"),
        0x9 => Some("NTSVCS"),
        0x0A => Some("NTRTL"),
        _ => None,
    }
}

/// 一笔段账。
#[derive(Clone)]
pub struct FaRec {
    pub seq: u32,
    pub fac: u32,
    pub seg: FacSeg,
    pub note: String,
}

impl FaRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 设施全表模型。
pub struct FacTable {
    pub entries: Vec<(u32, FacSeg, String)>,
    pub ledger: Vec<FaRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl FacTable {
    pub fn new() -> FacTable {
        FacTable { entries: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记核：段类判定 → 官方名对表 → 唯一。
    pub fn register(&mut self, fac: u32, customer_bit: u32, name: &str) -> Result<FacSeg, u32> {
        if self.sealed {
            return Err(FA_FROZEN);
        }
        if note_is_empty(name) {
            return Err(FA_BAD_NAME);
        }
        let seg = match seg_of(fac, customer_bit) {
            Some(s) => s,
            None => return Err(FA_RESERVED_PLAIN),
        };
        // 保留段直登即拒（须走客户位）。
        if seg == FacSeg::Reserved {
            self.ledger.push(FaRec {
                seq: self.seq_next,
                fac,
                seg,
                note: name.to_string(),
            });
            self.seq_next += 1;
            return Err(FA_RESERVED_PLAIN);
        }
        // 官方段名对表。
        if seg == FacSeg::Official {
            match official_name(fac) {
                Some(pinned) => {
                    if pinned != name {
                        return Err(FA_UNKNOWN_OFFICIAL);
                    }
                }
                None => return Err(FA_UNKNOWN_OFFICIAL),
            }
        }
        if self.entries.iter().any(|e| e.0 == fac) {
            return Err(FA_DUP);
        }
        let rec = FaRec { seq: self.seq_next, fac, seg, note: name.to_string() };
        if !rec.ok() {
            return Err(FA_BAD_NAME);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        self.entries.push((fac, seg, name.to_string()));
        Ok(seg)
    }

    /// 段清点。
    pub fn count_seg(&self, s: FacSeg) -> usize {
        self.entries.iter().filter(|e| e.1 == s).count()
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
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.0);
            h = h.wrapping_mul(211).wrapping_add(e.1 as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

fn note_is_empty(s: &str) -> bool {
    s.is_empty()
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = FacTable::new();
    // 1) 保留段直登（无客户位）→ FA_RESERVED_PLAIN。
    got[0] = t.register(0x200, 0, "RES").err() == Some(FA_RESERVED_PLAIN);
    // 2) 官方段错名 → FA_UNKNOWN_OFFICIAL。
    got[1] = t.register(0xB, 0, "WRONGNAME").err() == Some(FA_UNKNOWN_OFFICIAL);
    // 3) 官方段未钉号（0x50）→ FA_UNKNOWN_OFFICIAL。
    got[2] = t.register(0x50, 0, "NTCORE").err() == Some(FA_UNKNOWN_OFFICIAL);
    // 4) 空名 → FA_BAD_NAME。
    got[3] = t.register(0x900, 1, "").err() == Some(FA_BAD_NAME);
    // 5) 印账后登记 → FA_FROZEN。
    let _ = t.register(0x900, 1, "CUST1");
    t.seal();
    got[4] = t.register(0x901, 1, "CUST2").err() == Some(FA_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12038_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12038");

    // J1 段类判定：官方/保留/客户三段。
    let j1 = seg_of(0x0B, 0) == Some(FacSeg::Official)
        && seg_of(0x200, 0) == Some(FacSeg::Reserved)
        && seg_of(0x200, 1) == Some(FacSeg::Customer)
        && seg_of(0x900, 1) == Some(FacSeg::Customer);
    s.add("J1-三段判定", j1, "官方/保留/客户");

    // J2 官方钉定名表。
    let j2 = official_name(0x0) == Some("NTCORE")
        && official_name(0xB) == Some("NTDEBUG")
        && official_name(0x50).is_none();
    s.add("J2-钉定名表", j2, "0/0xB 在表 0x50 缺");

    // J3 官方段正登记。
    let j3 = {
        let mut t = FacTable::new();
        t.register(0x0, 0, "NTCORE") == Ok(FacSeg::Official)
            && t.register(0xB, 0, "NTDEBUG") == Ok(FacSeg::Official)
    };
    s.add("J3-官方登记", j3, "双官方入表");

    // J4 保留段直登拒且留账。
    let j4 = {
        let mut t = FacTable::new();
        let _ = t.register(0x300, 0, "RES");
        t.ledger.len() == 1 && t.entries.is_empty()
    };
    s.add("J4-保留直登拒", j4, "拒且入账不落表");

    // J5 客户段带位登记过。
    let j5 = {
        let mut t = FacTable::new();
        t.register(0x300, 1, "MYFAC") == Ok(FacSeg::Customer)
    };
    s.add("J5-客户位放行", j5, "同号带位即客户");

    // J6 官方错名拒。
    let j6 = {
        let mut t = FacTable::new();
        t.register(0xB, 0, "OTHER").err() == Some(FA_UNKNOWN_OFFICIAL)
    };
    s.add("J6-官方错名拒", j6, "对表精确");

    // J7 设施重复登记拒。
    let j7 = {
        let mut t = FacTable::new();
        let _ = t.register(0x0, 0, "NTCORE");
        t.register(0x0, 0, "NTCORE").err() == Some(FA_DUP)
    };
    s.add("J7-重复拒", j7, "设施唯一");

    // J8 空名拒。
    let j8 = {
        let mut t = FacTable::new();
        t.register(0x900, 1, "") == Err(FA_BAD_NAME)
    };
    s.add("J8-空名拒", j8, "名必填");

    // J9 段清点对平。
    let j9 = {
        let mut t = FacTable::new();
        let _ = t.register(0x0, 0, "NTCORE");
        let _ = t.register(0x900, 1, "C1");
        let _ = t.register(0x901, 1, "C2");
        t.count_seg(FacSeg::Official) == 1
            && t.count_seg(FacSeg::Customer) == 2
            && t.count_seg(FacSeg::Reserved) == 0
    };
    s.add("J9-段清点", j9, "1+2+0 对账");

    // J10 印账冻结。
    let j10 = {
        let mut t = FacTable::new();
        let _ = t.register(0x900, 1, "C1");
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2 && t.register(0x901, 1, "C2").err() == Some(FA_FROZEN)
    };
    s.add("J10-印账冻结", j10, "印后登记必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：混合全表。
    let mut t = FacTable::new();
    let _ = t.register(0x0, 0, "NTCORE");
    let _ = t.register(0x5, 0, "NTKERNEL");
    let _ = t.register(0xA00, 1, "MYDRV");
    s.add("样例三设施", t.entries.len() == 3, "2 官方 1 客户");
    s.add("样例账面合法", t.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12038_seg_flow() {
        let mut t = FacTable::new();
        assert_eq!(t.register(0xB, 0, "NTDEBUG"), Ok(FacSeg::Official));
        assert_eq!(t.register(0xB, 0, "NTDEBUG"), Err(FA_DUP));
        assert_eq!(t.register(0x200, 0, "RES"), Err(FA_RESERVED_PLAIN));
        assert_eq!(t.register(0x200, 1, "CUST"), Ok(FacSeg::Customer));
    }

    #[test]
    fn unx12038_checks_green() {
        let s = run_unx12038_checks();
        assert!(s.all_passed(), "UNX-F12038 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12038_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
