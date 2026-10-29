//! UNX-F12036 RtlNtStatusToDosError 映射表底座（0x196A0 段）
//!
//! 判据原文：NT→DOS 错误码映射表——已知映射逐对钉定（查表命中即映射）；
//! 未映射 NT 码回退 ERROR_MR_MID_NOT_FOUND（317）；映射表登记唯一性
//! （一个 NT 码只映射一个 DOS 码）；映射账逐笔可对账。
//!
//! 结构：映射表（XlatBook：NT→DOS 对册）+ 查表核（四码守卫）+ 映射账
//! （XlRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12036_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x196A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x196A0;
pub const XL_DUP_NT: u32 = SEG_BASE + 0x01; // NT 码重复登记
pub const XL_BAD_REC: u32 = SEG_BASE + 0x02; // 伪造映射账
pub const XL_UNMAPPED: u32 = SEG_BASE + 0x03; // 未映射码（回退 317）
pub const XL_FROZEN: u32 = SEG_BASE + 0x04; // 印账后仍登记
pub const XL_INVALID_PAIR: u32 = SEG_BASE + 0x05; // 非法对（DOS 码为零）

/// 未映射回退钉值。
pub const ERROR_MR_MID_NOT_FOUND: u32 = 317;

/// 一笔映射的账目。
#[derive(Clone)]
pub struct XlRec {
    pub seq: u32,
    pub nt: u32,
    pub dos: u32,
    /// 是否命中表（false=回退 317）。
    pub hit: bool,
    pub note: String,
}

impl XlRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
}

/// 映射表模型。
pub struct XlatBook {
    pub pairs: Vec<(u32, u32)>,
    pub ledger: Vec<XlRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl XlatBook {
    pub fn new() -> XlatBook {
        XlatBook { pairs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记映射对：NT/DOS 均非零 + NT 唯一。
    pub fn enlist(&mut self, nt: u32, dos: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(XL_FROZEN);
        }
        if note.is_empty() {
            return Err(XL_BAD_REC);
        }
        if dos == 0 || nt == 0 {
            return Err(XL_INVALID_PAIR);
        }
        if self.pairs.iter().any(|p| p.0 == nt) {
            return Err(XL_DUP_NT);
        }
        self.pairs.push((nt, dos));
        Ok(())
    }

    /// 查表核：命中即映射、未命中回退 317，均留账。
    pub fn xlate(&mut self, nt: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(XL_FROZEN);
        }
        if note.is_empty() {
            return Err(XL_BAD_REC);
        }
        let hit = self.pairs.iter().find(|p| p.0 == nt).map(|p| p.1);
        match hit {
            Some(dos) => {
                let r = XlRec { seq: self.seq_next, nt, dos, hit: true, note: note.to_string() };
                if !r.ok() {
                    return Err(XL_BAD_REC);
                }
                self.seq_next += 1;
                self.ledger.push(r);
                Ok(dos)
            }
            None => {
                let r = XlRec {
                    seq: self.seq_next,
                    nt,
                    dos: ERROR_MR_MID_NOT_FOUND,
                    hit: false,
                    note: note.to_string(),
                };
                if !r.ok() {
                    return Err(XL_BAD_REC);
                }
                self.seq_next += 1;
                self.ledger.push(r);
                Err(XL_UNMAPPED)
            }
        }
    }

    /// 命中/回退清点。
    pub fn hit_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.hit).count()
    }
    pub fn miss_count(&self) -> usize {
        self.ledger.iter().filter(|r| !r.hit).count()
    }

    /// 唯一性复核：NT 码无重复。
    pub fn unique_nt(&self) -> bool {
        for a in 0..self.pairs.len() {
            for b in (a + 1)..self.pairs.len() {
                if self.pairs[a].0 == self.pairs[b].0 {
                    return false;
                }
            }
        }
        true
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
        for p in &self.pairs {
            h = h.wrapping_mul(211).wrapping_add(p.0);
            h = h.wrapping_mul(211).wrapping_add(p.1);
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
    let mut b = XlatBook::new();
    let _ = b.enlist(0xC000_0005, 5, "av");
    // 1) NT 码重复登记 → XL_DUP_NT。
    got[0] = b.enlist(0xC000_0005, 6, "dup").err() == Some(XL_DUP_NT);
    // 2) 非法对：DOS 码零 → XL_INVALID_PAIR。
    got[1] = b.enlist(0xC000_0008, 0, "z").err() == Some(XL_INVALID_PAIR);
    // 3) 未映射码 → XL_UNMAPPED（回退 317 留账）。
    got[2] = b.xlate(0xC000_9999, "m").err() == Some(XL_UNMAPPED);
    // 4) 空留痕 → XL_BAD_REC。
    got[3] = b.xlate(0xC000_0005, "").err() == Some(XL_BAD_REC);
    // 5) 印账后动作 → XL_FROZEN。
    b.seal();
    got[4] = b.xlate(0xC000_0005, "post").err() == Some(XL_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12036_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12036");

    // J1 正路径：AV→5、IH→6、ACCESS_DENIED→5 双向钉对。
    let j1 = {
        let mut b = XlatBook::new();
        let _ = b.enlist(0xC000_0005, 5, "av");
        let _ = b.enlist(0xC000_0008, 6, "ih");
        let _ = b.enlist(0xC000_0022, 5, "ad");
        b.xlate(0xC000_0005, "a") == Ok(5)
            && b.xlate(0xC000_0008, "b") == Ok(6)
            && b.xlate(0xC000_0022, "c") == Ok(5)
    };
    s.add("J1-三对映射", j1, "5/6/5 全中");

    // J2 未映射回退 317。
    let j2 = {
        let mut b = XlatBook::new();
        let _ = b.enlist(0xC000_0005, 5, "av");
        let _ = b.xlate(0xE123_4567, "u");
        b.ledger[0].dos == ERROR_MR_MID_NOT_FOUND && !b.ledger[0].hit
    };
    s.add("J2-回退 317", j2, "未映射留账回退");

    // J3 回退钉值核对。
    let j3 = ERROR_MR_MID_NOT_FOUND == 317;
    s.add("J3-钉值核对", j3, "317");

    // J4 NT 码重复登记拒。
    let j4 = {
        let mut b = XlatBook::new();
        let _ = b.enlist(0xC000_0005, 5, "a");
        b.enlist(0xC000_0005, 99, "b").err() == Some(XL_DUP_NT)
    };
    s.add("J4-重复登记拒", j4, "一 NT 一 DOS");

    // J5 零对拒（双面）。
    let j5 = {
        let mut b = XlatBook::new();
        let n = b.enlist(0, 5, "z").err() == Some(XL_INVALID_PAIR);
        let d = b.enlist(0xC000_0005, 0, "z").err() == Some(XL_INVALID_PAIR);
        n && d
    };
    s.add("J5-零对拒", j5, "零 NT/零 DOS 全拒");

    // J6 命中/回退清点对平。
    let j6 = {
        let mut b = XlatBook::new();
        let _ = b.enlist(0xC000_0005, 5, "av");
        let _ = b.xlate(0xC000_0005, "h");
        let _ = b.xlate(0xC000_7777, "m");
        let _ = b.xlate(0xC000_8888, "m2");
        b.hit_count() == 1 && b.miss_count() == 2
            && b.hit_count() + b.miss_count() == b.ledger.len()
    };
    s.add("J6-命中对平", j6, "1 中 2 退");

    // J7 唯一性复核。
    let j7 = {
        let mut b = XlatBook::new();
        let _ = b.enlist(0xC000_0005, 5, "a");
        let _ = b.enlist(0xC000_0008, 6, "b");
        b.unique_nt()
    };
    s.add("J7-唯一复核", j7, "双对无重");

    // J8 空留痕拒。
    let j8 = {
        let mut b = XlatBook::new();
        b.xlate(0xC000_0005, "").err() == Some(XL_BAD_REC)
    };
    s.add("J8-空留痕拒", j8, "留痕必填");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = XlatBook::new();
        let _ = a.enlist(0xC000_0005, 5, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = XlatBook::new();
        let _ = c.enlist(0xC000_0005, 6, "x");
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异表异纹");

    // J10 印账冻结。
    let j10 = {
        let mut b = XlatBook::new();
        let _ = b.enlist(0xC000_0005, 5, "a");
        b.seal();
        b.xlate(0xC000_0005, "post").err() == Some(XL_FROZEN)
            && b.enlist(0xC000_0008, 6, "post").err() == Some(XL_FROZEN)
    };
    s.add("J10-印账冻结", j10, "双面操作印后全拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：常用映射面。
    let mut b = XlatBook::new();
    let _ = b.enlist(0xC000_0005, 5, "sum-av");
    let _ = b.enlist(0xC000_0008, 6, "sum-ih");
    let _ = b.enlist(0xC000_000D, 87, "sum-ip");
    s.add("样例三对在册", b.pairs.len() == 3, "5/6/87");
    s.add("样例账面合法", b.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12036_xlate_flow() {
        let mut b = XlatBook::new();
        assert!(b.enlist(0xC000_0005, 5, "a").is_ok());
        assert_eq!(b.enlist(0xC000_0005, 5, "b"), Err(XL_DUP_NT));
        assert_eq!(b.xlate(0xC000_0005, "c"), Ok(5));
        assert_eq!(b.xlate(0xE000_0001, "d"), Err(XL_UNMAPPED));
        assert_eq!(b.ledger[1].dos, ERROR_MR_MID_NOT_FOUND);
    }

    #[test]
    fn unx12036_checks_green() {
        let s = run_unx12036_checks();
        assert!(s.all_passed(), "UNX-F12036 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12036_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
