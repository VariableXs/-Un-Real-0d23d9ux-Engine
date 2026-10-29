//! UNX-F12039 NTSTATUS 对照表制度：语义档三件套母版（0x19700 段）
//!
//! 判据原文：对照表四列账——每个登记码挂四列（严重级/设施/短名/DOS
//! 映射），四列一致性为登记合法性：严重级与位域一致、设施与位域一致、
//! 短名非空、DOS 映射非零（无映射以 317 占位）；重码登记拒；四列对账
//! 逐码可复核。
//!
//! 结构：对照册（XrefBook：码 × 四列）+ 登记核（四码守卫）+ 复核面
//! （逐码四列对账）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12039_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19700 独占 0x20）。
pub const SEG_BASE: u32 = 0x19700;
pub const XR_SEV_MISMATCH: u32 = SEG_BASE + 0x01; // 严重级列与位域不符
pub const XR_FAC_MISMATCH: u32 = SEG_BASE + 0x02; // 设施列与位域不符
pub const XR_DUP_CODE: u32 = SEG_BASE + 0x03; // 重码登记
pub const XR_BAD_COLUMNS: u32 = SEG_BASE + 0x04; // 名空/DOS 零
pub const XR_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍登记

/// 回退占位。
pub const DOS_FALLBACK: u32 = 317;

/// 对照行四列。
#[derive(Clone)]
pub struct XrefRow {
    pub code: u32,
    /// 严重级声明列。
    pub severity: u32,
    /// 设施声明列。
    pub facility: u32,
    pub name: String,
    pub dos: u32,
}

impl XrefRow {
    /// 四列一致性复核。
    pub fn columns_ok(&self) -> bool {
        self.severity == self.code >> 30
            && self.facility == (self.code >> 16) & 0xFFF
            && !self.name.is_empty()
            && self.dos != 0
    }
}

/// 一笔复核的账目。
#[derive(Clone)]
pub struct XrRec {
    pub seq: u32,
    pub code: u32,
    pub consistent: bool,
    pub note: String,
}

impl XrRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 对照册模型。
pub struct XrefBook {
    pub rows: Vec<XrefRow>,
    pub ledger: Vec<XrRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl XrefBook {
    pub fn new() -> XrefBook {
        XrefBook { rows: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记核：四列预检 + 重码拒。
    pub fn enlist(
        &mut self,
        code: u32,
        severity: u32,
        facility: u32,
        name: &str,
        dos: u32,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(XR_FROZEN);
        }
        if name.is_empty() || dos == 0 {
            return Err(XR_BAD_COLUMNS);
        }
        if severity != code >> 30 {
            return Err(XR_SEV_MISMATCH);
        }
        if facility != (code >> 16) & 0xFFF {
            return Err(XR_FAC_MISMATCH);
        }
        if self.rows.iter().any(|r| r.code == code) {
            return Err(XR_DUP_CODE);
        }
        self.rows.push(XrefRow {
            code,
            severity,
            facility,
            name: name.to_string(),
            dos,
        });
        Ok(())
    }

    /// 复核核：逐行四列对账留痕。
    pub fn audit(&mut self, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(XR_FROZEN);
        }
        if note.is_empty() {
            return Err(XR_BAD_COLUMNS);
        }
        let mut bad = 0usize;
        for r in self.rows.iter() {
            let ok = r.columns_ok();
            if !ok {
                bad += 1;
            }
            self.ledger.push(XrRec {
                seq: self.seq_next,
                code: r.code,
                consistent: ok,
                note: note.to_string(),
            });
            self.seq_next += 1;
        }
        if bad > 0 {
            Err(XR_BAD_COLUMNS)
        } else {
            Ok(self.rows.len())
        }
    }

    /// 行数。
    pub fn count(&self) -> usize {
        self.rows.len()
    }

    /// 全表一致性（无账面直核）。
    pub fn all_consistent(&self) -> bool {
        self.rows.iter().all(|r| r.columns_ok())
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
        for r in &self.rows {
            h = h.wrapping_mul(211).wrapping_add(r.code);
            h = h.wrapping_mul(211).wrapping_add(r.dos);
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
    let mut b = XrefBook::new();
    // 1) 严重级列不符：0xC0000005 声明 severity=0 → XR_SEV_MISMATCH。
    got[0] = b
        .enlist(0xC000_0005, 0, 0x000, "ACCESS_VIOLATION", 5)
        .err() == Some(XR_SEV_MISMATCH);
    // 2) 设施列不符：声明 facility=1（位域为 0）→ XR_FAC_MISMATCH。
    got[1] = b
        .enlist(0xC000_0005, 3, 0x001, "ACCESS_VIOLATION", 5)
        .err() == Some(XR_FAC_MISMATCH);
    // 3) 重码登记 → XR_DUP_CODE。
    let _ = b.enlist(0xC000_0008, 3, 0x000, "INVALID_HANDLE", 6);
    got[2] = b
        .enlist(0xC000_0008, 3, 0x000, "INVALID_HANDLE", 6)
        .err() == Some(XR_DUP_CODE);
    // 4) DOS 零列 → XR_BAD_COLUMNS。
    got[3] = b
        .enlist(0xC000_000D, 3, 0x000, "INVALID_PARAMETER", 0)
        .err() == Some(XR_BAD_COLUMNS);
    // 5) 印账后登记 → XR_FROZEN。
    b.seal();
    got[4] = b
        .enlist(0xC000_000D, 3, 0x000, "INVALID_PARAMETER", 87)
        .err() == Some(XR_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12039_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12039");

    // J1 正路径：三码四列登记全过复核。
    let j1 = {
        let mut b = XrefBook::new();
        let _ = b.enlist(0xC000_0005, 3, 0x000, "ACCESS_VIOLATION", 5);
        let _ = b.enlist(0xC000_0008, 3, 0x000, "INVALID_HANDLE", 6);
        let _ = b.enlist(0x0000_0103, 0, 0x000, "PENDING", DOS_FALLBACK);
        b.all_consistent() && b.count() == 3
    };
    s.add("J1-三码四列", j1, "全过复核");

    // J2 四列一致性逐列核对。
    let j2 = {
        let r = XrefRow {
            code: 0xC123_4567,
            severity: 3,
            facility: 0x123,
            name: "TEST".to_string(),
            dos: 1,
        };
        r.columns_ok()
    };
    s.add("J2-四列一致", j2, "位域逐列对");

    // J3 严重级列不符拒。
    let j3 = {
        let mut b = XrefBook::new();
        b.enlist(0xC000_0005, 0, 0, "AV", 5).err() == Some(XR_SEV_MISMATCH)
    };
    s.add("J3-严重级拒", j3, "声明≠位域");

    // J4 设施列不符拒。
    let j4 = {
        let mut b = XrefBook::new();
        b.enlist(0xC123_4567, 3, 0x456, "AV", 5).err() == Some(XR_FAC_MISMATCH)
    };
    s.add("J4-设施列拒", j4, "声明≠位域");

    // J5 重码拒。
    let j5 = {
        let mut b = XrefBook::new();
        let _ = b.enlist(0xC000_0005, 3, 0, "AV", 5);
        b.enlist(0xC000_0005, 3, 0, "AV2", 6).err() == Some(XR_DUP_CODE)
    };
    s.add("J5-重码拒", j5, "码唯一");

    // J6 名空/DOS 零拒。
    let j6 = {
        let mut b = XrefBook::new();
        let n = b.enlist(0xC000_0005, 3, 0, "", 5) == Err(XR_BAD_COLUMNS);
        let d = b.enlist(0xC000_0008, 3, 0, "IH", 0) == Err(XR_BAD_COLUMNS);
        n && d
    };
    s.add("J6-坏列拒", j6, "双面拒");

    // J7 回退占位合法：DOS=317 过列。
    let j7 = {
        let mut b = XrefBook::new();
        let _ = b.enlist(0x0000_0103, 0, 0x000, "PENDING", DOS_FALLBACK);
        b.rows[0].dos == 317 && b.all_consistent()
    };
    s.add("J7-回退占位", j7, "317 合法列");

    // J8 复核账：一致行 consistent=true。
    let j8 = {
        let mut b = XrefBook::new();
        let _ = b.enlist(0xC000_0005, 3, 0, "AV", 5);
        let _ = b.audit("a");
        b.ledger.len() == 1 && b.ledger[0].consistent && b.ledger_ok()
    };
    s.add("J8-复核账", j8, "一致留痕");

    // J9 空留痕复核拒。
    let j9 = {
        let mut b = XrefBook::new();
        b.audit("").err() == Some(XR_BAD_COLUMNS)
    };
    s.add("J9-空痕复核拒", j9, "留痕必填");

    // J10 印账冻结。
    let j10 = {
        let mut b = XrefBook::new();
        let _ = b.enlist(0xC000_0005, 3, 0, "AV", 5);
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2
            && b.enlist(0xC000_0008, 3, 0, "IH", 6).err() == Some(XR_FROZEN)
            && b.audit("post").err() == Some(XR_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：四码母版。
    let mut b = XrefBook::new();
    let _ = b.enlist(0xC000_0005, 3, 0x000, "ACCESS_VIOLATION", 5);
    let _ = b.enlist(0xC000_0008, 3, 0x000, "INVALID_HANDLE", 6);
    let _ = b.enlist(0xC000_000D, 3, 0x000, "INVALID_PARAMETER", 87);
    let _ = b.enlist(0x0000_0000, 0, 0x000, "SUCCESS", 0 + 0 + DOS_FALLBACK);
    s.add("样例四码母版", b.count() == 4 && b.all_consistent(), "四列全对");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12039_xref_flow() {
        let mut b = XrefBook::new();
        assert!(b.enlist(0xC000_0005, 3, 0, "AV", 5).is_ok());
        assert_eq!(b.enlist(0xC000_0005, 3, 0, "AV", 5), Err(XR_DUP_CODE));
        assert_eq!(b.enlist(0xC000_0005, 1, 0, "AV", 5), Err(XR_SEV_MISMATCH));
        assert_eq!(b.audit("a"), Ok(1));
    }

    #[test]
    fn unx12039_checks_green() {
        let s = run_unx12039_checks();
        assert!(s.all_passed(), "UNX-F12039 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12039_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
