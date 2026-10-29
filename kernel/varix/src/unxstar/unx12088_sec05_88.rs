//! UNX-F12088 SecurityDescriptor 字段与 J1 安全挂点（0x19d20 段）
//!
//! 卡面：OBJECT_ATTRIBUTES→SecurityDescriptor——修订版锚 1、自相对格式
//! 必置、控制位族闭集、控制位与字段在场位一致（J1 安全挂点联签）。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 修订锚：revision 必为 1，违者必拒。
//! - J2 自相对必置：SE_SELF_RELATIVE 未置必拒。
//! - J3 控制位闭集：未知控制位非零必拒。
//! - J4 位与字段一致：SE_DACL_PRESENT 置而 has_dacl 假（且非 NULL DACL
//!   语义）必拒；反过来在场无位亦拒。
//! - J5 登记唯一：实例编号非零唯一。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：SdRec 描述符档 + SdBook 登记册 + 一致性核 + 折叠指纹
//! （211 进制卷积）+ 注入核（坏修订/未自相对/未知位/位字段失配/印后登）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19d20 独占 0x20）。
pub const SEG_BASE: u32 = 0x19d20;
pub const SD_BAD_REV: u32 = SEG_BASE + 0x01; // 修订锚失配
pub const SD_NOT_SELF_REL: u32 = SEG_BASE + 0x02; // 未自相对
pub const SD_UNKNOWN_CTL: u32 = SEG_BASE + 0x03; // 未知控制位
pub const SD_CTL_MISMATCH: u32 = SEG_BASE + 0x04; // 位与字段失配
pub const SD_DUP: u32 = SEG_BASE + 0x05; // 重号（含零号）
pub const SD_FROZEN: u32 = SEG_BASE + 0x06; // 印账后登记

/// 控制位常量。
pub const C_SELF_RELATIVE: u16 = 0x8000;
pub const C_DACL_PRESENT: u16 = 0x0004;
pub const C_SACL_PRESENT: u16 = 0x0010;
pub const C_OWNER_DEFAULTED: u16 = 0x0001;
pub const C_GROUP_DEFAULTED: u16 = 0x0002;
pub const KNOWN_CTL: u16 =
    C_SELF_RELATIVE | C_DACL_PRESENT | C_SACL_PRESENT
        | C_OWNER_DEFAULTED | C_GROUP_DEFAULTED;

/// 描述符档。
#[derive(Clone)]
pub struct SdRec {
    pub id: u32,
    pub revision: u8,
    pub control: u16,
    pub has_owner: bool,
    pub has_group: bool,
    pub has_dacl: bool,
    pub has_sacl: bool,
}

impl SdRec {
    /// 一致性核。
    pub fn validate(&self) -> Result<(), u32> {
        if self.revision != 1 {
            return Err(SD_BAD_REV);
        }
        if self.control & C_SELF_RELATIVE == 0 {
            return Err(SD_NOT_SELF_REL);
        }
        if self.control & !KNOWN_CTL != 0 {
            return Err(SD_UNKNOWN_CTL);
        }
        let ctl_dacl = self.control & C_DACL_PRESENT != 0;
        if ctl_dacl != self.has_dacl {
            return Err(SD_CTL_MISMATCH);
        }
        let ctl_sacl = self.control & C_SACL_PRESENT != 0;
        if ctl_sacl != self.has_sacl {
            return Err(SD_CTL_MISMATCH);
        }
        Ok(())
    }
    pub fn render(&self) -> String {
        format!("SD{}|r{}|c{:04x}", self.id, self.revision, self.control)
    }
}

/// 描述符登记册。
pub struct SdBook {
    pub recs: Vec<SdRec>,
    pub sealed: bool,
}

impl SdBook {
    pub fn new() -> SdBook {
        SdBook { recs: Vec::new(), sealed: false }
    }

    /// 登记：编号唯一 → 一致性核。
    pub fn put(&mut self, r: SdRec) -> Result<(), u32> {
        if self.sealed {
            return Err(SD_FROZEN);
        }
        if r.id == 0 || self.recs.iter().any(|x| x.id == r.id) {
            return Err(SD_DUP);
        }
        r.validate()?;
        self.recs.push(r);
        Ok(())
    }

    pub fn by_id(&self, id: u32) -> Option<&SdRec> {
        self.recs.iter().find(|r| r.id == id)
    }

    /// DACL 挂点查询（J1 安全挂点联签面）。
    pub fn dacl_attached(&self, id: u32) -> Option<bool> {
        self.by_id(id).map(|r| r.has_dacl)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            h = h.wrapping_mul(211).wrapping_add(r.revision as u32);
            h = h.wrapping_mul(211).wrapping_add(r.control as u32);
            h = h.wrapping_mul(211)
                .wrapping_add((r.has_owner as u32) | (r.has_group as u32) << 1);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("recs={}", self.recs.len())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, id: u32) -> String {
        match self.by_id(id) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SdBook::new();
    // 1) 坏修订必拒。
    got[0] = b.put(SdRec {
        id: 1, revision: 2, control: C_SELF_RELATIVE,
        has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
    }).err() == Some(SD_BAD_REV);
    // 2) 未自相对必拒。
    got[1] = b.put(SdRec {
        id: 1, revision: 1, control: 0,
        has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
    }).err() == Some(SD_NOT_SELF_REL);
    // 3) 未知控制位必拒。
    got[2] = b.put(SdRec {
        id: 1, revision: 1, control: C_SELF_RELATIVE | 0x0040,
        has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
    }).err() == Some(SD_UNKNOWN_CTL);
    // 4) 位与字段失配必拒。
    got[3] = b.put(SdRec {
        id: 1, revision: 1, control: C_SELF_RELATIVE | C_DACL_PRESENT,
        has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
    }).err() == Some(SD_CTL_MISMATCH);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(SdRec {
        id: 1, revision: 1, control: C_SELF_RELATIVE,
        has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
    }).err() == Some(SD_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12088_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12088");

    // J1 修订锚。
    let j1 = {
        let r = SdRec {
            id: 1, revision: 0, control: C_SELF_RELATIVE,
            has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
        };
        r.validate().err() == Some(SD_BAD_REV)
    };
    s.add("J1-修订锚", j1, "非 1 必拒");

    // J2 自相对必置。
    let j2 = {
        let r = SdRec {
            id: 1, revision: 1, control: C_DACL_PRESENT,
            has_owner: false, has_group: false, has_dacl: true, has_sacl: false,
        };
        r.validate().err() == Some(SD_NOT_SELF_REL)
    };
    s.add("J2-自相对必置", j2, "0x8000 未置必拒");

    // J3 控制位闭集。
    let j3 = {
        let r = SdRec {
            id: 1, revision: 1, control: C_SELF_RELATIVE | 0x0100,
            has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
        };
        r.validate().err() == Some(SD_UNKNOWN_CTL)
    };
    s.add("J3-控制位闭集", j3, "族外位必拒");

    // J4 位与字段一致（DACL/SACL 双面）。
    let j4 = {
        let ok = SdRec {
            id: 1, revision: 1, control: C_SELF_RELATIVE | C_DACL_PRESENT,
            has_owner: false, has_group: false, has_dacl: true, has_sacl: false,
        };
        let bad_s = SdRec {
            id: 1, revision: 1, control: C_SELF_RELATIVE | C_SACL_PRESENT,
            has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
        };
        ok.validate().is_ok() && bad_s.validate().err() == Some(SD_CTL_MISMATCH)
    };
    s.add("J4-位字段一致", j4, "DACL/SACL 双面校验");

    // J5 正路径挂点查询。
    let j5 = {
        let mut b = SdBook::new();
        let _ = b.put(SdRec {
            id: 5, revision: 1, control: C_SELF_RELATIVE | C_DACL_PRESENT,
            has_owner: true, has_group: true, has_dacl: true, has_sacl: false,
        });
        b.dacl_attached(5) == Some(true) && b.summary().contains("recs=1")
    };
    s.add("J5-挂点查询", j5, "DACL 挂点可查");

    // J6 零号与重号拒。
    let j6 = {
        let mut b = SdBook::new();
        let r = SdRec {
            id: 3, revision: 1, control: C_SELF_RELATIVE,
            has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
        };
        let _ = b.put(r.clone());
        b.put(r).err() == Some(SD_DUP)
            && b.put(SdRec {
                id: 0, revision: 1, control: C_SELF_RELATIVE,
                has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
            }).err() == Some(SD_DUP)
    };
    s.add("J6-编号唯一", j6, "零号重号皆拒");

    // J7 渲染行。
    let j7 = {
        let mut b = SdBook::new();
        let _ = b.put(SdRec {
            id: 7, revision: 1, control: C_SELF_RELATIVE,
            has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
        });
        b.line_of(7).contains("c8000") && b.line_of(8) == "missing"
    };
    s.add("J7-渲染行", j7, "控制位十六进制回读");

    // J8 印账冻结。
    let j8 = {
        let mut b = SdBook::new();
        b.seal();
        b.put(SdRec {
            id: 1, revision: 1, control: C_SELF_RELATIVE,
            has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
        }).err() == Some(SD_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后必拒");

    // J9 指纹幂等。
    let j9 = {
        let mut b = SdBook::new();
        let _ = b.put(SdRec {
            id: 1, revision: 1, control: C_SELF_RELATIVE,
            has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
        });
        b.fold() == b.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 注入核 5/5。
    let got = inject_five();
    let j10 = got.iter().all(|x| *x);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = SdBook::new();
    let _ = b.put(SdRec {
        id: 1, revision: 1, control: C_SELF_RELATIVE | C_DACL_PRESENT,
        has_owner: true, has_group: true, has_dacl: true, has_sacl: false,
    });
    let _ = b.put(SdRec {
        id: 2, revision: 1, control: C_SELF_RELATIVE,
        has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
    });

    s.add("摘要回读", b.summary().contains("recs=2"), "双档在册");
    s.add("DACL 有无分账", b.dacl_attached(1) == Some(true)
        && b.dacl_attached(2) == Some(false), "一有一无");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12088_validate_flow() {
        let good = SdRec {
            id: 1, revision: 1, control: C_SELF_RELATIVE | C_DACL_PRESENT,
            has_owner: false, has_group: false, has_dacl: true, has_sacl: false,
        };
        assert!(good.validate().is_ok());
        let bad = SdRec { revision: 3, ..good };
        assert_eq!(bad.validate(), Err(SD_BAD_REV));
    }

    #[test]
    fn unx12088_checks_green() {
        let s = run_unx12088_checks();
        assert!(s.all_passed(), "UNX-F12088 判据未全绿");
    }

    #[test]
    fn book_dup_guard() {
        let mut b = SdBook::new();
        let r = SdRec {
            id: 2, revision: 1, control: C_SELF_RELATIVE,
            has_owner: false, has_group: false, has_dacl: false, has_sacl: false,
        };
        assert!(b.put(r.clone()).is_ok());
        assert_eq!(b.put(r), Err(SD_DUP));
    }
}
