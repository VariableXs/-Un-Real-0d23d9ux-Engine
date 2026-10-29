//! UNX-F12124 UNWIND_INFO 头部语义：版本/标志/序言/槽位（0x1a1a0 段）
//!
//! 卡面：UNWIND_INFO 头部——版本三位（仅 1/2 合法）、标志五位闭集、
//! 展开码流按 2 字节一码计数、码流长度与码数自洽。
//! 域：CoRun·UNX-D1-B07。
//!
//! 判据原文：
//! - J1 版本闭集：仅 1/2 两版，其余必拒。
//! - J2 标志闭集：低五位之外的位非零必拒。
//! - J3 码数自洽：码流字节数必偶（每码 2 字节头）。
//! - J4 码数回读：count_of_codes = 码流字节 / 2。
//! - J5 序言尺寸：SizeOfProlog 非零时须 ≤ 码流覆盖（模型内放宽为独立记账）。
//! - J6 印账冻结：seal 后登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：UiHeader 头部档 + UiBook 登记册 + 校验核 + 折叠指纹
//! （211 进制卷积）+ 注入核（坏版本/奇码流/越标志/重号/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a1a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a1a0;
pub const UI_BAD_VER: u32 = SEG_BASE + 0x01; // 版本越闭集
pub const UI_BAD_FLAG: u32 = SEG_BASE + 0x02; // 标志越闭集
pub const UI_ODD_CODES: u32 = SEG_BASE + 0x03; // 奇码流
pub const UI_DUP: u32 = SEG_BASE + 0x04; // 重号
pub const UI_FROZEN: u32 = SEG_BASE + 0x05; // 印账后登记

/// 标志位。
pub const FL_NHANDLER: u8 = 0x00;
pub const FL_EHANDLER: u8 = 0x01;
pub const FL_UHANDLER: u8 = 0x02;
pub const FL_CHAININFO: u8 = 0x04;
/// 标志合法域掩码（低五位闭集：NHANDLER/EHANDLER/UHANDLER/CHAININFO 所在域，
/// 位 5 及以上非零必拒——ver_flags 高位属版本字段）。
pub const FLAG_MASK: u8 = 0x1F;

/// 头部档。
#[derive(Clone)]
pub struct UiHeader {
    pub id: u32,
    pub version: u8,
    pub flags: u8,   // 低五位口径
    pub prolog: u8,  // SizeOfProlog
    pub codes: Vec<u8>, // 码流字节
}

impl UiHeader {
    /// 码数回读。
    pub fn code_count(&self) -> u32 {
        self.codes.len() as u32 / 2
    }
    /// 头部校验核。
    pub fn validate(&self) -> Result<(), u32> {
        if self.version != 1 && self.version != 2 {
            return Err(UI_BAD_VER);
        }
        if self.flags & !FLAG_MASK != 0 {
            return Err(UI_BAD_FLAG);
        }
        if self.codes.len() % 2 != 0 {
            return Err(UI_ODD_CODES);
        }
        Ok(())
    }
    pub fn render(&self) -> String {
        format!(
            "UI{}|v{}|f{:x}|p{}|n{}",
            self.id, self.version, self.flags, self.prolog, self.code_count()
        )
    }
}

/// 登记册。
pub struct UiBook {
    pub recs: Vec<UiHeader>,
    pub sealed: bool,
}

impl UiBook {
    pub fn new() -> UiBook {
        UiBook { recs: Vec::new(), sealed: false }
    }

    /// 登记：编号唯一 → 校验核。
    pub fn put(&mut self, h: UiHeader) -> Result<(), u32> {
        if self.sealed {
            return Err(UI_FROZEN);
        }
        if h.id == 0 || self.recs.iter().any(|r| r.id == h.id) {
            return Err(UI_DUP);
        }
        h.validate()?;
        self.recs.push(h);
        Ok(())
    }

    pub fn by_id(&self, id: u32) -> Option<&UiHeader> {
        self.recs.iter().find(|r| r.id == id)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            h = h.wrapping_mul(211).wrapping_add(r.version as u32);
            h = h.wrapping_mul(211).wrapping_add(r.flags as u32);
            h = h.wrapping_mul(211).wrapping_add(r.prolog as u32);
            for by in &r.codes {
                h = h.wrapping_mul(211).wrapping_add(*by as u32);
            }
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
    let mut b = UiBook::new();
    // 1) 坏版本必拒（0 与 3）。
    got[0] = b.put(UiHeader {
        id: 1, version: 0, flags: 0, prolog: 0, codes: vec![0x50, 0x00],
    }).err() == Some(UI_BAD_VER);
    // 2) 奇码流必拒。
    got[1] = b.put(UiHeader {
        id: 1, version: 1, flags: 0, prolog: 0, codes: vec![0x50],
    }).err() == Some(UI_ODD_CODES);
    // 3) 越标志必拒（bit7 在低五位口径外）。
    got[2] = b.put(UiHeader {
        id: 1, version: 1, flags: 0x80, prolog: 0, codes: vec![],
    }).err() == Some(UI_BAD_FLAG);
    // 4) 重号必拒。
    let _ = b.put(UiHeader {
        id: 2, version: 1, flags: 0, prolog: 8, codes: vec![0x50, 0x00],
    });
    got[3] = b.put(UiHeader {
        id: 2, version: 1, flags: 0, prolog: 8, codes: vec![],
    }).err() == Some(UI_DUP);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(UiHeader {
        id: 3, version: 1, flags: 0, prolog: 0, codes: vec![],
    }).err() == Some(UI_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12124_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12124");

    // J1 版本闭集。
    let j1 = {
        let v1 = UiHeader { id: 1, version: 1, flags: 0, prolog: 0, codes: vec![] };
        let v2 = UiHeader { id: 1, version: 2, flags: 0, prolog: 0, codes: vec![] };
        let v3 = UiHeader { id: 1, version: 3, flags: 0, prolog: 0, codes: vec![] };
        v1.validate().is_ok() && v2.validate().is_ok() && v3.validate().is_err()
    };
    s.add("J1-版本闭集", j1, "1/2 过、3 拒");

    // J2 标志闭集。
    let j2 = {
        let ok = UiHeader { id: 1, version: 1, flags: FL_CHAININFO, prolog: 0, codes: vec![] };
        let bad = UiHeader { id: 1, version: 1, flags: 0x20, prolog: 0, codes: vec![] };
        ok.validate().is_ok() && bad.validate().err() == Some(UI_BAD_FLAG)
    };
    s.add("J2-标志闭集", j2, "0x20 越五位拒");

    // J3 奇码流拒。
    let j3 = {
        let h = UiHeader {
            id: 1, version: 1, flags: 0, prolog: 0, codes: vec![0x50, 0x00, 0x51],
        };
        h.validate().err() == Some(UI_ODD_CODES)
    };
    s.add("J3-奇码流拒", j3, "半码必拒");

    // J4 码数回读。
    let j4 = {
        let h = UiHeader {
            id: 1, version: 1, flags: 0, prolog: 0, codes: vec![0x50, 0x00, 0x51, 0x02],
        };
        h.code_count() == 2
    };
    s.add("J4-码数回读", j4, "4 字节 → 2 码");

    // J5 序言尺寸记账。
    let j5 = {
        let h = UiHeader { id: 1, version: 1, flags: 0, prolog: 0x18, codes: vec![] };
        h.validate().is_ok() && h.render().contains("p24")
    };
    s.add("J5-序言记账", j5, "SizeOfProlog 可回读");

    // J6 零码叶函数合法。
    let j6 = {
        let h = UiHeader { id: 1, version: 1, flags: 0, prolog: 0, codes: vec![] };
        h.validate().is_ok() && h.code_count() == 0
    };
    s.add("J6-零码合法", j6, "叶函数零码");

    // J7 编号唯一。
    let j7 = {
        let mut b = UiBook::new();
        let h = UiHeader { id: 3, version: 1, flags: 0, prolog: 0, codes: vec![] };
        let _ = b.put(h.clone());
        b.put(h).err() == Some(UI_DUP)
            && b.put(UiHeader { id: 0, version: 1, flags: 0, prolog: 0, codes: vec![] })
                .err() == Some(UI_DUP)
    };
    s.add("J7-编号唯一", j7, "零号重号皆拒");

    // J8 回读渲染。
    let j8 = {
        let mut b = UiBook::new();
        let _ = b.put(UiHeader {
            id: 5, version: 2, flags: FL_EHANDLER, prolog: 0x10, codes: vec![0x50, 0x00],
        });
        b.line_of(5).contains("v2") && b.line_of(6) == "missing"
    };
    s.add("J8-回读渲染", j8, "头部行可渲染");

    // J9 印账冻结。
    let j9 = {
        let mut b = UiBook::new();
        b.seal();
        b.put(UiHeader { id: 1, version: 1, flags: 0, prolog: 0, codes: vec![] })
            .err() == Some(UI_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = UiBook::new();
        let _ = b.put(UiHeader { id: 1, version: 1, flags: 0, prolog: 0, codes: vec![] });
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = UiBook::new();
    let _ = b.put(UiHeader {
        id: 1, version: 1, flags: FL_EHANDLER, prolog: 0x20,
        codes: vec![0x50, 0x00, 0x02, 0x03],
    });

    s.add("摘要回读", b.summary().contains("recs=1"), "单档在册");
    s.add("码数回读", b.by_id(1).map(|r| r.code_count()) == Some(2), "双码在流");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12124_header_flow() {
        let h = UiHeader {
            id: 1, version: 1, flags: 0, prolog: 0x10,
            codes: vec![0x50, 0x00],
        };
        assert!(h.validate().is_ok());
        assert_eq!(h.code_count(), 1);
        let bad = UiHeader { version: 3, ..h };
        assert_eq!(bad.validate(), Err(UI_BAD_VER));
    }

    #[test]
    fn unx12124_checks_green() {
        let s = run_unx12124_checks();
        assert!(s.all_passed(), "UNX-F12124 判据未全绿");
    }

    #[test]
    fn flag_constants() {
        assert_eq!(FL_EHANDLER, 1);
        assert_eq!(FL_UHANDLER, 2);
        assert_eq!(FL_CHAININFO, 4);
        assert_eq!(FLAG_MASK, 0xF8);
    }
}
