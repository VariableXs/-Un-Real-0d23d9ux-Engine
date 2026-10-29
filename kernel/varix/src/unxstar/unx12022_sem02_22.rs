//! UNX-F12022 成功族语义档：STATUS_SUCCESS 与 INFORMATIONAL 档（0x194E0 段）
//!
//! 判据原文：成功族二档——严重级 0（STATUS_SUCCESS 与同族正成功码）与
//! 严重级 1（INFORMATIONAL 提示档，如 STATUS_OBJECT_NAME_EXISTS）；档
//! 判定须逐码过位域；档外码混入成功族册即红账；成功族册中每个登记码
//! 的严重级回读必须与档一致。
//!
//! 结构：档判核（classify：位域严重级切分）+ 成功族册（二档闭集登记 +
//! 档一致性对账）+ 登记账（SfRec 留痕）+ 折叠指纹 + 印账冻结 + 注入核 +
//! 判据聚合（run_unx12022_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x194E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x194E0;
pub const SF_TIER_MISMATCH: u32 = SEG_BASE + 0x01; // 档外码混入
pub const SF_DUP: u32 = SEG_BASE + 0x02; // 重号登记
pub const SF_BAD_REC: u32 = SEG_BASE + 0x03; // 伪造账目（空留痕）
pub const SF_TIER_CONFLICT: u32 = SEG_BASE + 0x04; // 档声明与位域冲突
pub const SF_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍登记

/// 严重级位域读取。
pub fn severity_bits(v: u32) -> u32 {
    v >> 30
}

/// 档闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuccessTier {
    Success = 0,
    Informational = 1,
}

/// 档判核。
pub fn classify(v: u32) -> Option<SuccessTier> {
    match severity_bits(v) {
        0 => Some(SuccessTier::Success),
        1 => Some(SuccessTier::Informational),
        _ => None,
    }
}

/// 成功族登记项。
#[derive(Clone)]
pub struct SfEntry {
    pub code: u32,
    /// 声明档。
    pub tier: SuccessTier,
    pub note: String,
}

impl SfEntry {
    /// 档一致性：声明档必须与位域严重级一致。
    pub fn tier_ok(&self) -> bool {
        classify(self.code) == Some(self.tier)
    }
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.tier_ok()
    }
}

/// 成功族册模型。
pub struct SuccessBook {
    pub entries: Vec<SfEntry>,
    pub sealed: bool,
}

impl SuccessBook {
    pub fn new() -> SuccessBook {
        SuccessBook { entries: Vec::new(), sealed: false }
    }

    /// 登记核：档一致 + 唯一 + 非空留痕。
    pub fn register(&mut self, code: u32, tier: SuccessTier, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SF_FROZEN);
        }
        if note.is_empty() {
            return Err(SF_BAD_REC);
        }
        if classify(code) != Some(tier) {
            return Err(SF_TIER_MISMATCH);
        }
        if self.entries.iter().any(|e| e.code == code) {
            return Err(SF_DUP);
        }
        let e = SfEntry { code, tier, note: note.to_string() };
        if !e.ok() {
            return Err(SF_BAD_REC);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 档清点。
    pub fn count_tier(&self, t: SuccessTier) -> usize {
        self.entries.iter().filter(|e| e.tier == t).count()
    }

    /// 全册档一致复核。
    pub fn all_tier_ok(&self) -> bool {
        self.entries.iter().all(|e| e.tier_ok())
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.entries.iter().all(|e| e.ok())
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
            h = h.wrapping_mul(211).wrapping_add(e.code);
            h = h.wrapping_mul(211).wrapping_add(e.tier as u32);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = SuccessBook::new();
    let _ = b.register(0x0000_0000, SuccessTier::Success, "ok");
    // 1) 档外码混入：Error 级 0xC000_0005 冒充 Success → SF_TIER_MISMATCH。
    got[0] =
        b.register(0xC000_0005, SuccessTier::Success, "err").err() == Some(SF_TIER_MISMATCH);
    // 2) 档声明与位域冲突：Success 码冒充 Informational → SF_TIER_MISMATCH
    //    （同一守卫、反向声明）。
    got[1] = b
        .register(0x0000_0001, SuccessTier::Informational, "swap")
        .err() == Some(SF_TIER_MISMATCH);
    // 3) 重号登记 → SF_DUP。
    got[2] = b.register(0x0000_0000, SuccessTier::Success, "dup").err() == Some(SF_DUP);
    // 4) 伪造账目：空留痕登记项自洽核红 → SF_BAD_REC。
    got[3] = b.register(0x4000_0015, SuccessTier::Informational, "").err() == Some(SF_BAD_REC);
    // 5) 印账后登记 → SF_FROZEN。
    b.seal();
    got[4] =
        b.register(0x4000_0015, SuccessTier::Informational, "post").err() == Some(SF_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12022_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12022");

    // J1 STATUS_SUCCESS 档判。
    let j1 = classify(0x0000_0000) == Some(SuccessTier::Success);
    s.add("J1-SUCCESS 档", j1, "零码入成功档");

    // J2 正成功码同档：STATUS_WAIT_0=1 亦严重级 0。
    let j2 = classify(0x0000_0001) == Some(SuccessTier::Success)
        && classify(0x0000_00BC) == Some(SuccessTier::Success);
    s.add("J2-正成功档", j2, "WAIT_0/ABANDONED 同档");

    // J3 INFORMATIONAL 档：0x40000000 级段。
    let j3 = classify(0x4000_0000) == Some(SuccessTier::Informational)
        && classify(0x4000_0015) == Some(SuccessTier::Informational);
    s.add("J3-INFO 档", j3, "0x4 级段入档");

    // J4 档外码判 None：Warning/Error 全拒。
    let j4 = classify(0x8000_0001).is_none() && classify(0xC000_0005).is_none();
    s.add("J4-档外判空", j4, "2/3 级不入成功族");

    // J5 登记正路径：双档各一码。
    let j5 = {
        let mut b = SuccessBook::new();
        let _ = b.register(0x0000_0000, SuccessTier::Success, "succ");
        let _ = b.register(0x4000_0015, SuccessTier::Informational, "info");
        b.count_tier(SuccessTier::Success) == 1 && b.count_tier(SuccessTier::Informational) == 1
    };
    s.add("J5-双档登记", j5, "各档一码入册");

    // J6 档一致复核：全册声明档与位域一致。
    let j6 = {
        let mut b = SuccessBook::new();
        let _ = b.register(0x0000_00BC, SuccessTier::Success, "a");
        let _ = b.register(0x4000_0000, SuccessTier::Informational, "b");
        b.all_tier_ok() && b.ledger_ok()
    };
    s.add("J6-档一致复核", j6, "逐码过档核");

    // J7 档外混入拒（双面）。
    let j7 = {
        let mut b = SuccessBook::new();
        let e = b.register(0xC000_0005, SuccessTier::Success, "x").err() == Some(SF_TIER_MISMATCH);
        let w = b.register(0x8000_0001, SuccessTier::Informational, "y").err() == Some(SF_TIER_MISMATCH);
        e && w
    };
    s.add("J7-档外混入拒", j7, "Error/Warning 冒充必拒");

    // J8 反向声明拒。
    let j8 = {
        let mut b = SuccessBook::new();
        b.register(0x0000_0001, SuccessTier::Informational, "z").err() == Some(SF_TIER_MISMATCH)
    };
    s.add("J8-反向声明拒", j8, "档互换必拒");

    // J9 重号登记拒。
    let j9 = {
        let mut b = SuccessBook::new();
        let _ = b.register(0, SuccessTier::Success, "a");
        b.register(0, SuccessTier::Success, "b").err() == Some(SF_DUP)
    };
    s.add("J9-重号拒", j9, "码唯一");

    // J10 空留痕拒。
    let j10 = {
        let mut b = SuccessBook::new();
        b.register(0x0000_0001, SuccessTier::Success, "").err() == Some(SF_BAD_REC)
    };
    s.add("J10-空留痕拒", j10, "留痕必填");

    // J11 印账冻结。
    let j11 = {
        let mut b = SuccessBook::new();
        let _ = b.register(0, SuccessTier::Success, "a");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2
            && b.register(0x4000_0000, SuccessTier::Informational, "post").err() == Some(SF_FROZEN)
    };
    s.add("J11-指纹封账", j11, "指纹幂等且印后必拒");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例：典型成功族册。
    let mut b = SuccessBook::new();
    let _ = b.register(0x0000_0000, SuccessTier::Success, "STATUS_SUCCESS");
    let _ = b.register(0x0000_0001, SuccessTier::Success, "STATUS_WAIT_0");
    let _ = b.register(0x4000_0015, SuccessTier::Informational, "REPARSE");
    s.add("样例三码在册", b.entries.len() == 3, "双档三码");
    s.add("样例账面合法", b.ledger_ok(), "逐码过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12022_classify_flow() {
        assert_eq!(classify(0), Some(SuccessTier::Success));
        assert_eq!(classify(0x4000_0000), Some(SuccessTier::Informational));
        assert_eq!(classify(0xC000_0005), None);
        let mut b = SuccessBook::new();
        assert!(b.register(0, SuccessTier::Success, "a").is_ok());
        assert_eq!(b.register(0xC000_0005, SuccessTier::Success, "b"), Err(SF_TIER_MISMATCH));
    }

    #[test]
    fn unx12022_checks_green() {
        let s = run_unx12022_checks();
        assert!(s.all_passed(), "UNX-F12022 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12022_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
