//! UNX-F12101 UNICODE_STRING 结构语义实装（0x1a140→0x19ec0 段）
//!
//! 卡面：UNICODE_STRING 三字段结构（Length/MaximumLength/Buffer）——
//! 长不超容、长为偶数（UTF-16 字节口径）、缓冲在场与容量互证。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 长不超容：Length ≤ MaximumLength，违者必拒。
//! - J2 长为偶数：UTF-16 字节长必为偶数。
//! - J3 缓冲互证：容量为 0 时缓冲不得在场；容量非 0 时缓冲必须在场。
//! - J4 容量上限：MaximumLength ≤ 0xFFFE（NUL 位留容）。
//! - J5 登记唯一：实例编号非零唯一。
//! - J6 回读渲染：逐实例字段可回读。
//! - J7 印账冻结：seal 后登记必拒。
//! - J8 注入五况全抓。
//!
//! 结构：UniStr 结构档 + UsBook 登记册 + 校验核 + 折叠指纹（211 进制
//! 卷积）+ 注入核（长超容/奇长/缓冲失证/超容量/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19ec0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19ec0;
pub const US_LEN_GT_MAX: u32 = SEG_BASE + 0x01; // 长超容
pub const US_ODD_LEN: u32 = SEG_BASE + 0x02; // 奇长
pub const US_BUF_MISMATCH: u32 = SEG_BASE + 0x03; // 缓冲失证
pub const US_MAX_OVER: u32 = SEG_BASE + 0x04; // 超容量上限
pub const US_DUP_ID: u32 = SEG_BASE + 0x05; // 重号（含零号）
pub const US_FROZEN: u32 = SEG_BASE + 0x06; // 印账后登记

/// 容量上限（NUL 位留容）。
pub const US_MAX_CAP: u32 = 0xFFFE;

/// UNICODE_STRING 结构档。
#[derive(Clone)]
pub struct UniStr {
    pub id: u32,
    pub len: u32,
    pub max: u32,
    pub buf_present: bool,
    pub tag: String,
}

impl UniStr {
    /// 结构静态校验：容量 → 长容序 → 偶长 → 缓冲互证。
    pub fn validate(&self) -> Result<(), u32> {
        if self.max > US_MAX_CAP {
            return Err(US_MAX_OVER);
        }
        if self.len > self.max {
            return Err(US_LEN_GT_MAX);
        }
        if self.len % 2 != 0 {
            return Err(US_ODD_LEN);
        }
        if self.max == 0 && self.buf_present {
            return Err(US_BUF_MISMATCH);
        }
        if self.max > 0 && !self.buf_present {
            return Err(US_BUF_MISMATCH);
        }
        Ok(())
    }
    pub fn render(&self) -> String {
        format!("U{}|{}+{}", self.id, self.len, self.max)
    }
}

/// 登记册。
pub struct UsBook {
    pub recs: Vec<UniStr>,
    pub sealed: bool,
}

impl UsBook {
    pub fn new() -> UsBook {
        UsBook { recs: Vec::new(), sealed: false }
    }

    /// 登记：编号唯一 → 校验核。
    pub fn put(&mut self, u: UniStr) -> Result<(), u32> {
        if self.sealed {
            return Err(US_FROZEN);
        }
        if u.id == 0 || self.recs.iter().any(|r| r.id == u.id) {
            return Err(US_DUP_ID);
        }
        u.validate()?;
        self.recs.push(u);
        Ok(())
    }

    pub fn by_id(&self, id: u32) -> Option<&UniStr> {
        self.recs.iter().find(|r| r.id == id)
    }

    /// 空串合法（len=0, max=2, buf 在场）。
    pub fn put_empty(&mut self, id: u32, tag: &str) -> Result<(), u32> {
        self.put(UniStr {
            id, len: 0, max: 2, buf_present: true, tag: tag.to_string(),
        })
    }

    /// 全册自洽。
    pub fn all_ok(&self) -> bool {
        self.recs.iter().all(|r| r.validate().is_ok())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            h = h.wrapping_mul(211).wrapping_add(r.len);
            h = h.wrapping_mul(211).wrapping_add(r.max);
            h = h.wrapping_mul(211).wrapping_add(r.buf_present as u32);
            for by in r.tag.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
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
    let mut b = UsBook::new();
    // 1) 长超容必拒。
    got[0] = b.put(UniStr {
        id: 1, len: 16, max: 8, buf_present: true, tag: "a".to_string(),
    }).err() == Some(US_LEN_GT_MAX);
    // 2) 奇长必拒。
    got[1] = b.put(UniStr {
        id: 1, len: 7, max: 16, buf_present: true, tag: "b".to_string(),
    }).err() == Some(US_ODD_LEN);
    // 3) 缓冲失证必拒（有容无缓冲）。
    got[2] = b.put(UniStr {
        id: 1, len: 0, max: 16, buf_present: false, tag: "c".to_string(),
    }).err() == Some(US_BUF_MISMATCH);
    // 4) 超容量上限必拒。
    got[3] = b.put(UniStr {
        id: 1, len: 0, max: 0x1_0000, buf_present: true, tag: "d".to_string(),
    }).err() == Some(US_MAX_OVER);
    // 5) 印账后登记必抓。
    b.seal();
    got[4] = b.put(UniStr {
        id: 1, len: 0, max: 2, buf_present: true, tag: "p".to_string(),
    }).err() == Some(US_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12101_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12101");

    // J1 长不超容。
    let j1 = {
        let u = UniStr {
            id: 1, len: 16, max: 8, buf_present: true, tag: "x".to_string(),
        };
        u.validate().err() == Some(US_LEN_GT_MAX)
    };
    s.add("J1-长不超容", j1, "超容必拒");

    // J2 偶长约束。
    let j2 = {
        let odd = UniStr {
            id: 1, len: 7, max: 16, buf_present: true, tag: "x".to_string(),
        };
        let even = UniStr {
            id: 1, len: 8, max: 16, buf_present: true, tag: "x".to_string(),
        };
        odd.validate().err() == Some(US_ODD_LEN) && even.validate().is_ok()
    };
    s.add("J2-偶长约束", j2, "奇拒偶过");

    // J3 缓冲互证：双向。
    let j3 = {
        let no_buf = UniStr {
            id: 1, len: 0, max: 16, buf_present: false, tag: "x".to_string(),
        };
        let orphan_buf = UniStr {
            id: 1, len: 0, max: 0, buf_present: true, tag: "x".to_string(),
        };
        no_buf.validate().err() == Some(US_BUF_MISMATCH)
            && orphan_buf.validate().err() == Some(US_BUF_MISMATCH)
    };
    s.add("J3-缓冲互证", j3, "双向失证皆拒");

    // J4 容量上限。
    let j4 = {
        let u = UniStr {
            id: 1, len: 0, max: US_MAX_CAP, buf_present: true, tag: "x".to_string(),
        };
        let over = UniStr { max: US_MAX_CAP + 2, ..u };
        over.validate().err() == Some(US_MAX_OVER)
    };
    s.add("J4-容量上限", j4, "越 0xFFFE 必拒");

    // J5 空串合法。
    let j5 = {
        let mut b = UsBook::new();
        b.put_empty(1, "empty").is_ok()
    };
    s.add("J5-空串合法", j5, "len0/max2/buf 在场");

    // J6 编号唯一。
    let j6 = {
        let mut b = UsBook::new();
        let u = UniStr {
            id: 3, len: 0, max: 2, buf_present: true, tag: "a".to_string(),
        };
        let _ = b.put(u.clone());
        b.put(u).err() == Some(US_DUP_ID)
            && b.put(UniStr {
                id: 0, len: 0, max: 2, buf_present: true, tag: "z".to_string(),
            }).err() == Some(US_DUP_ID)
    };
    s.add("J6-编号唯一", j6, "零号重号皆拒");

    // J7 回读渲染。
    let j7 = {
        let mut b = UsBook::new();
        let _ = b.put(UniStr {
            id: 5, len: 8, max: 16, buf_present: true, tag: "n".to_string(),
        });
        b.line_of(5).contains("8+16") && b.line_of(6) == "missing"
    };
    s.add("J7-回读渲染", j7, "字段可回读");

    // J8 全册自洽。
    let j8 = {
        let mut b = UsBook::new();
        let _ = b.put_empty(1, "a");
        let _ = b.put(UniStr {
            id: 2, len: 8, max: 16, buf_present: true, tag: "b".to_string(),
        });
        b.all_ok() && b.summary().contains("recs=2")
    };
    s.add("J8-全册自洽", j8, "逐档过校验");

    // J9 印账冻结。
    let j9 = {
        let mut b = UsBook::new();
        b.seal();
        b.put_empty(1, "p").err() == Some(US_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = UsBook::new();
        let _ = b.put_empty(1, "a");
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = UsBook::new();
    let _ = b.put(UniStr {
        id: 1, len: 12, max: 32, buf_present: true, tag: "path".to_string(),
    });
    let _ = b.put_empty(2, "nil");

    s.add("摘要回读", b.summary().contains("recs=2"), "双档在册");
    s.add("指纹非零", b.fold() != 0, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12101_validate_flow() {
        let good = UniStr {
            id: 1, len: 8, max: 16, buf_present: true, tag: "a".to_string(),
        };
        assert!(good.validate().is_ok());
        let bad = UniStr { len: 20, ..good };
        assert_eq!(bad.validate(), Err(US_LEN_GT_MAX));
    }

    #[test]
    fn unx12101_checks_green() {
        let s = run_unx12101_checks();
        assert!(s.all_passed(), "UNX-F12101 判据未全绿");
    }

    #[test]
    fn book_dup_and_render() {
        let mut b = UsBook::new();
        let u = UniStr {
            id: 4, len: 0, max: 2, buf_present: true, tag: "e".to_string(),
        };
        assert!(b.put(u.clone()).is_ok());
        assert_eq!(b.put(u), Err(US_DUP_ID));
        assert!(b.line_of(4).contains("U4"));
    }
}
