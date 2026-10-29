//! UNX-F12025 STATUS_ACCESS_DENIED 访问拒绝族与安全联签预告（0x19540 段）
//!
//! 判据原文：访问检查以位掩码面进行——期望访问（desired）必须落在
//! 合法掩码域内；超出已授面（granted）的位即 ACCESS_DENIED
//! （0xC0000022）并逐位留拒账；零期望访问不作检查；拒账逐笔可对账。
//!
//! 结构：访问检查器（AceChecker：合法域 + 已授面）+ 检查核（五码守卫）
//! + 拒账（AdRec 逐位留痕）+ 折叠指纹 + 印账冻结 + 注入核 + 判据聚合
//! （run_unx12025_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19540 独占 0x20）。
pub const SEG_BASE: u32 = 0x19540;
pub const AD_INVALID_MASK: u32 = SEG_BASE + 0x01; // 期望越合法域
pub const AD_DENIED: u32 = SEG_BASE + 0x02; // 拒面命中
pub const AD_ZERO_DESIRE: u32 = SEG_BASE + 0x03; // 零期望
pub const AD_BAD_REC: u32 = SEG_BASE + 0x04; // 伪造拒账
pub const AD_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍动作

/// STATUS_ACCESS_DENIED 语义钉值。
pub const STATUS_ACCESS_DENIED: u32 = 0xC000_0022;

/// 一笔访问检查的账目。
#[derive(Clone)]
pub struct AdRec {
    pub seq: u32,
    pub desired: u32,
    pub denied_bits: u32,
    pub granted: bool,
    pub note: String,
}

impl AdRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
    /// 拒账自洽：拒面位必须全部落在合法域内。
    pub fn denied_in_valid(&self, valid: u32) -> bool {
        self.denied_bits & !valid == 0
    }
}

/// 访问检查器。
pub struct AceChecker {
    pub valid_mask: u32,
    pub granted_mask: u32,
    pub ledger: Vec<AdRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl AceChecker {
    pub fn new(valid_mask: u32, granted_mask: u32) -> AceChecker {
        AceChecker {
            valid_mask,
            granted_mask: granted_mask & valid_mask,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 检查核：零期望拒 / 越域拒 / 拒面留账。
    pub fn check(&mut self, desired: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(AD_FROZEN);
        }
        if note.is_empty() {
            return Err(AD_INVALID_MASK);
        }
        if desired == 0 {
            return Err(AD_ZERO_DESIRE);
        }
        if desired & !self.valid_mask != 0 {
            return Err(AD_INVALID_MASK);
        }
        let denied_bits = desired & !self.granted_mask;
        let granted = denied_bits == 0;
        let rec = AdRec {
            seq: self.seq_next,
            desired,
            denied_bits,
            granted,
            note: note.to_string(),
        };
        if !rec.ok() || !rec.denied_in_valid(self.valid_mask) {
            return Err(AD_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        if granted {
            Ok(())
        } else {
            Err(AD_DENIED)
        }
    }

    /// 拒账清点。
    pub fn denied_count(&self) -> usize {
        self.ledger.iter().filter(|r| !r.granted).count()
    }
    pub fn granted_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.granted).count()
    }

    /// 拒面并集：所有拒账位聚合。
    pub fn denied_union(&self) -> u32 {
        self.ledger.iter().fold(0, |acc, r| acc | r.denied_bits)
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok() && r.denied_in_valid(self.valid_mask))
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.valid_mask);
        h = h.wrapping_mul(211).wrapping_add(self.granted_mask);
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut c = AceChecker::new(0x001F_00FF, 0x0001_0000);
    // 1) 期望越合法域 → AD_INVALID_MASK。
    got[0] = c.check(0x0100_0000, "wild").err() == Some(AD_INVALID_MASK);
    // 2) 拒面命中：期望含未授位 → AD_DENIED。
    got[1] = c.check(0x0001_0001, "deny").err() == Some(AD_DENIED);
    // 3) 零期望 → AD_ZERO_DESIRE。
    got[2] = c.check(0, "z").err() == Some(AD_ZERO_DESIRE);
    // 4) 伪造拒账：拒面位越合法域自洽核红 → AD_BAD_REC 面。
    let forged = AdRec {
        seq: 1,
        desired: 1,
        denied_bits: 0x0100_0000,
        granted: false,
        note: "f".to_string(),
    };
    got[3] = !forged.denied_in_valid(0x001F_00FF);
    // 5) 印账后检查 → AD_FROZEN。
    c.seal();
    got[4] = c.check(0x0001_0000, "post").err() == Some(AD_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12025_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12025");

    // J1 正路径：已授位内检查放行。
    let j1 = {
        let mut c = AceChecker::new(0x001F_00FF, 0x0012_0000);
        c.check(0x0012_0000, "ok").is_ok() && c.granted_count() == 1
    };
    s.add("J1-授位放行", j1, "全授面检查过");

    // J2 拒面命中：部分授位部分拒位。
    let j2 = {
        let mut c = AceChecker::new(0xFF, 0x0F);
        c.check(0x10, "deny").err() == Some(AD_DENIED)
    };
    s.add("J2-拒面命中", j2, "未授位必拒");

    // J3 拒账逐位留痕：拒面精确记录。
    let j3 = {
        let mut c = AceChecker::new(0xFF, 0x0F);
        let _ = c.check(0x30, "d");
        c.ledger[0].denied_bits == 0x30 && !c.ledger[0].granted
    };
    s.add("J3-拒面留痕", j3, "0x30 全记拒");

    // J4 拒面并集聚合。
    let j4 = {
        let mut c = AceChecker::new(0xFF, 0x01);
        let _ = c.check(0x02, "a");
        let _ = c.check(0x04, "b");
        c.denied_union() == 0x06
    };
    s.add("J4-拒面并集", j4, "0x02|0x04=0x06");

    // J5 期望越合法域拒。
    let j5 = {
        let mut c = AceChecker::new(0xFF, 0xFF);
        c.check(0x100, "wild").err() == Some(AD_INVALID_MASK)
    };
    s.add("J5-越域拒", j5, "合法域外必拒");

    // J6 零期望拒。
    let j6 = {
        let mut c = AceChecker::new(0xFF, 0xFF);
        c.check(0, "z").err() == Some(AD_ZERO_DESIRE)
    };
    s.add("J6-零期望拒", j6, "无欲无查");

    // J7 已授面钳制：granted 超出 valid 被钳到域内。
    let j7 = {
        let c = AceChecker::new(0x0F, 0xFF);
        c.granted_mask == 0x0F
    };
    s.add("J7-授面钳制", j7, "授面不越合法域");

    // J8 账目对平：放行+拒收==总账。
    let j8 = {
        let mut c = AceChecker::new(0xFF, 0x0F);
        let _ = c.check(0x01, "g");
        let _ = c.check(0x20, "d");
        c.granted_count() + c.denied_count() == c.ledger.len()
    };
    s.add("J8-账目对平", j8, "一放一拒");

    // J9 账目完整性：拒面位恒落合法域。
    let j9 = {
        let mut c = AceChecker::new(0xFF, 0x0F);
        let _ = c.check(0xF0, "d");
        c.ledger_ok()
    };
    s.add("J9-账目自洽", j9, "拒面在域内");

    // J10 印账冻结。
    let j10 = {
        let mut c = AceChecker::new(0xFF, 0x0F);
        let f1 = c.fold();
        let f2 = c.fold();
        c.seal();
        f1 == f2 && c.check(0x01, "post").err() == Some(AD_FROZEN)
    };
    s.add("J10-指纹封账", j10, "指纹幂等且印后必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：文件访问面（读/写/执行域）。
    let mut c = AceChecker::new(0x0012_0001, 0x0010_0000);
    let _ = c.check(0x0010_0000, "read");
    let _ = c.check(0x0012_0001, "rwx");
    s.add("样例一放一拒", c.granted_count() == 1 && c.denied_count() == 1, "读通写拒");
    s.add("样例账面合法", c.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12025_ace_flow() {
        let mut c = AceChecker::new(0xFF, 0x0F);
        assert!(c.check(0x01, "g").is_ok());
        assert_eq!(c.check(0x10, "d"), Err(AD_DENIED));
        assert_eq!(c.check(0, "z"), Err(AD_ZERO_DESIRE));
        assert_eq!(c.check(0x100, "w"), Err(AD_INVALID_MASK));
    }

    #[test]
    fn unx12025_checks_green() {
        let s = run_unx12025_checks();
        assert!(s.all_passed(), "UNX-F12025 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12025_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
