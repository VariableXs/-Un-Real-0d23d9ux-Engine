//! UNX-F12122 RUNTIME_FUNCTION 三元组语义：Begin/End/UnwindData（0x1a160 段）
//!
//! 卡面：RUNTIME_FUNCTION 十二字节三元组——BeginAddress/EndAddress/
//! UnwindData；区间有序、尺寸上限、UnwindData 四对齐、零地址拒。
//! 域：CoRun·UNX-D1-B07。
//!
//! 判据原文：
//! - J1 区间有序：Begin < End，违者必拒。
//! - J2 尺寸上限：End − Begin ≤ 0x40000（256KB 单目上限）。
//! - J3 UnwindData 对齐：四字节对齐，违者必拒。
//! - J4 零地址拒：BeginAddress 为 0 必拒。
//! - J5 三元组唯一：同 Begin 二次登记必拒。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：Rf3 三元组 + RfBook 登记册 + 校验核 + 折叠指纹（211 进制
//! 卷积）+ 注入核（零地址/逆区间/未对齐/重目/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a160 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a160;
pub const RF_ZERO: u32 = SEG_BASE + 0x01; // 零地址
pub const RF_BAD_RANGE: u32 = SEG_BASE + 0x02; // 逆区间
pub const RF_TOO_BIG: u32 = SEG_BASE + 0x03; // 超尺寸
pub const RF_UNALIGNED: u32 = SEG_BASE + 0x04; // Unwind 未对齐
pub const RF_DUP: u32 = SEG_BASE + 0x05; // 重目
pub const RF_FROZEN: u32 = SEG_BASE + 0x06; // 印账后登记

/// 单目尺寸上限。
pub const RF_MAX_FUNC: u32 = 0x40000;

/// RUNTIME_FUNCTION 三元组。
#[derive(Clone, Copy)]
pub struct Rf3 {
    pub begin: u32,
    pub end: u32,
    pub unwind: u32,
}

impl Rf3 {
    /// 三元组静态校验。
    pub fn validate(&self) -> Result<(), u32> {
        if self.begin == 0 {
            return Err(RF_ZERO);
        }
        if self.begin >= self.end {
            return Err(RF_BAD_RANGE);
        }
        if self.end - self.begin > RF_MAX_FUNC {
            return Err(RF_TOO_BIG);
        }
        if self.unwind % 4 != 0 {
            return Err(RF_UNALIGNED);
        }
        Ok(())
    }
    /// 函数跨度。
    pub fn span(&self) -> u32 {
        if self.end > self.begin {
            self.end - self.begin
        } else {
            0
        }
    }
    pub fn render(&self) -> String {
        format!("F{:x}-{:x}|u{:x}", self.begin, self.end, self.unwind)
    }
}

/// 登记册。
pub struct RfBook {
    pub recs: Vec<Rf3>,
    pub sealed: bool,
}

impl RfBook {
    pub fn new() -> RfBook {
        RfBook { recs: Vec::new(), sealed: false }
    }

    /// 登记：校验 → 唯一。
    pub fn put(&mut self, f: Rf3) -> Result<(), u32> {
        if self.sealed {
            return Err(RF_FROZEN);
        }
        f.validate()?;
        if self.recs.iter().any(|r| r.begin == f.begin) {
            return Err(RF_DUP);
        }
        self.recs.push(f);
        Ok(())
    }

    /// 表目数。
    pub fn count(&self) -> usize {
        self.recs.len()
    }

    /// 跨度合计。
    pub fn span_sum(&self) -> u32 {
        self.recs.iter().map(|r| r.span()).sum()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.begin);
            h = h.wrapping_mul(211).wrapping_add(r.end);
            h = h.wrapping_mul(211).wrapping_add(r.unwind);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("recs={} span={:x}", self.count(), self.span_sum())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, i: usize) -> String {
        match self.recs.get(i) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = RfBook::new();
    // 1) 零地址必拒。
    got[0] = b.put(Rf3 { begin: 0, end: 0x10, unwind: 0x20 }).err() == Some(RF_ZERO);
    // 2) 逆区间必拒。
    got[1] = b.put(Rf3 { begin: 0x20, end: 0x10, unwind: 0x30 }).err() == Some(RF_BAD_RANGE);
    // 3) Unwind 未对齐必拒。
    got[2] = b.put(Rf3 { begin: 0x10, end: 0x20, unwind: 0x22 }).err() == Some(RF_UNALIGNED);
    // 4) 重目必拒。
    let _ = b.put(Rf3 { begin: 0x1000, end: 0x1100, unwind: 0x2000 });
    got[3] = b.put(Rf3 { begin: 0x1000, end: 0x1200, unwind: 0x2100 }).err() == Some(RF_DUP);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(Rf3 { begin: 0x2000, end: 0x2100, unwind: 0x2200 }).err() == Some(RF_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12122_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12122");

    // J1 区间有序。
    let j1 = {
        let ok = Rf3 { begin: 0x100, end: 0x200, unwind: 0x300 };
        let bad = Rf3 { begin: 0x200, end: 0x200, unwind: 0x300 };
        ok.validate().is_ok() && bad.validate().err() == Some(RF_BAD_RANGE)
    };
    s.add("J1-区间有序", j1, "Begin==End 亦拒");

    // J2 零地址拒。
    let j2 = {
        let f = Rf3 { begin: 0, end: 8, unwind: 0x10 };
        f.validate().err() == Some(RF_ZERO)
    };
    s.add("J2-零地址拒", j2, "Begin 0 必拒");

    // J3 尺寸上限：边界过、越限拒。
    let j3 = {
        let edge = Rf3 { begin: 0x10, end: 0x10 + RF_MAX_FUNC, unwind: 0x20 };
        let over = Rf3 { begin: 0x10, end: 0x10 + RF_MAX_FUNC + 4, unwind: 0x20 };
        edge.validate().is_ok() && over.validate().err() == Some(RF_TOO_BIG)
    };
    s.add("J3-尺寸上限", j3, "0x40000 边界");

    // J4 Unwind 对齐。
    let j4 = {
        let mis = Rf3 { begin: 0x10, end: 0x20, unwind: 0x26 };
        mis.validate().err() == Some(RF_UNALIGNED)
    };
    s.add("J4-Unwind对齐", j4, "非四对齐必拒");

    // J5 跨度回读。
    let j5 = {
        let f = Rf3 { begin: 0x1000, end: 0x1050, unwind: 0x2000 };
        f.span() == 0x50 && f.render().contains("1050")
    };
    s.add("J5-跨度回读", j5, "End−Begin 可查");

    // J6 登记与重目拒。
    let j6 = {
        let mut b = RfBook::new();
        let _ = b.put(Rf3 { begin: 0x1000, end: 0x1100, unwind: 0x2000 });
        b.put(Rf3 { begin: 0x1000, end: 0x1200, unwind: 0x2100 }).err() == Some(RF_DUP)
            && b.count() == 1
    };
    s.add("J6-重目拒", j6, "同 Begin 二次必拒");

    // J7 跨度合计。
    let j7 = {
        let mut b = RfBook::new();
        let _ = b.put(Rf3 { begin: 0x1000, end: 0x1100, unwind: 0x2000 });
        let _ = b.put(Rf3 { begin: 0x1200, end: 0x1300, unwind: 0x2100 });
        b.span_sum() == 0x200 && b.summary().contains("span=200")
    };
    s.add("J7-跨度合计", j7, "双目 0x200");

    // J8 渲染行。
    let j8 = {
        let mut b = RfBook::new();
        let _ = b.put(Rf3 { begin: 0x401000, end: 0x401040, unwind: 0x50000 });
        b.line_of(0).contains("401040") && b.line_of(1) == "missing"
    };
    s.add("J8-渲染行", j8, "三元组行可渲染");

    // J9 印账冻结。
    let j9 = {
        let mut b = RfBook::new();
        b.seal();
        b.put(Rf3 { begin: 0x10, end: 0x20, unwind: 0x30 }).err() == Some(RF_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = RfBook::new();
        let _ = b.put(Rf3 { begin: 0x10, end: 0x20, unwind: 0x30 });
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = RfBook::new();
    let _ = b.put(Rf3 { begin: 0x401000, end: 0x401080, unwind: 0x50000 });
    let _ = b.put(Rf3 { begin: 0x4010a0, end: 0x4010c0, unwind: 0x50100 });

    s.add("摘要回读", b.summary().contains("recs=2"), "双目在册");
    s.add("指纹非零", b.fold() != 0, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12122_validate_flow() {
        let good = Rf3 { begin: 0x1000, end: 0x1100, unwind: 0x2000 };
        assert!(good.validate().is_ok());
        assert_eq!(good.span(), 0x100);
        let bad = Rf3 { begin: 0, end: 8, unwind: 0x10 };
        assert_eq!(bad.validate(), Err(RF_ZERO));
    }

    #[test]
    fn unx1222_checks_green_fallback() {
        let s = run_unx12122_checks();
        assert!(s.all_passed(), "UNX-F12122 判据未全绿");
    }

    #[test]
    fn book_dup_guard() {
        let mut b = RfBook::new();
        let f = Rf3 { begin: 0x10, end: 0x20, unwind: 0x30 };
        assert!(b.put(f).is_ok());
        assert_eq!(b.put(f), Err(RF_DUP));
    }
}
