//! UNX-F12118 UNICODE_STRING 对照表制度与样本集（0x1a0e0 段）
//!
//! 卡面：UNICODE_STRING 逐字段对照表（Length@0/MaximumLength@2/
//! Buffer@8，x64 结构 16 字节）+ 样本集账（串样本登记供复测）。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 三字段闭集：Length/MaximumLength/Buffer 三档一一归档。
//! - J2 区间不重叠：新档起点不落入前档区间。
//! - J3 对齐落位：字段偏移 8 字节对齐（x64 口径）。
//! - J4 总跨度：尾档不越 16 字节结构锚。
//! - J5 样本登记：样本编号非零唯一且长为偶。
//! - J6 印账冻结：seal 后归档与样本登记必拒。
//! - J7 注入五况全抓。
//!
//! 结构：UsField 字段档 + UsXrefBook 对照册 + UsSample 样本账 +
//! 断言核 + 折叠指纹（211 进制卷积）+
//! 注入核（未对齐/重叠/超锚/重样本/印后归档）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a0e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a0e0;
pub const X2_UNALIGNED: u32 = SEG_BASE + 0x01; // 未对齐
pub const X2_OVERLAP: u32 = SEG_BASE + 0x02; // 重叠
pub const X2_EXCEED: u32 = SEG_BASE + 0x03; // 超锚
pub const X2_DUP_SAMPLE: u32 = SEG_BASE + 0x04; // 重样本（含零号）
pub const X2_FROZEN: u32 = SEG_BASE + 0x05; // 印账后操作

/// x64 UNICODE_STRING 结构锚 16 字节。
pub const US_STRUCT_ANCHOR: u32 = 16;

/// 字段档。
#[derive(Clone)]
pub struct UsField {
    pub name: String,
    pub off: u32,
    pub size: u32,
}

impl UsField {
    pub fn end(&self) -> u32 {
        self.off + self.size
    }
    pub fn render(&self) -> String {
        format!("F{}|{:x}+{:x}", self.name, self.off, self.size)
    }
}

/// 样本账。
#[derive(Clone)]
pub struct UsSample {
    pub id: u32,
    pub nchars: u32,
    pub tag: String,
}

impl UsSample {
    /// 样本推导长度口径：len = n×2、max = len+2。
    pub fn derived(&self) -> (u32, u32) {
        (self.nchars * 2, self.nchars * 2 + 2)
    }
    pub fn render(&self) -> String {
        let (l, m) = self.derived();
        format!("S{}|n{}|{}+{}", self.id, self.nchars, l, m)
    }
}

/// 对照册 + 样本账。
pub struct UsXrefBook {
    pub fields: Vec<UsField>,
    pub samples: Vec<UsSample>,
    pub sealed: bool,
}

impl UsXrefBook {
    pub fn new() -> UsXrefBook {
        UsXrefBook { fields: Vec::new(), samples: Vec::new(), sealed: false }
    }

    /// 字段对齐口径：8 字节档（size ≥ 8）须 8 对齐；小字段（size < 8，u16 口径）
    /// 须 2 对齐——x64 真结构：Length@0、MaximumLength@2 为 u16，Buffer@8 为指针对齐。
    pub fn field_align(size: u32) -> u32 {
        if size >= 8 {
            8
        } else {
            2
        }
    }

    /// 归档字段：重叠 → 对齐 → 超锚 → 零尺寸。
    pub fn archive(&mut self, f: UsField) -> Result<(), u32> {
        if self.sealed {
            return Err(X2_FROZEN);
        }
        if let Some(prev) = self.fields.last() {
            if f.off < prev.end() {
                return Err(X2_OVERLAP);
            }
        }
        if f.off % Self::field_align(f.size) != 0 {
            return Err(X2_UNALIGNED);
        }
        if f.end() > US_STRUCT_ANCHOR {
            return Err(X2_EXCEED);
        }
        if f.size == 0 {
            return Err(X2_UNALIGNED); // 零尺寸按对齐面显性拒
        }
        self.fields.push(f);
        Ok(())
    }

    /// 样本登记：编号唯一（含零号）。
    pub fn add_sample(&mut self, id: u32, nchars: u32, tag: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(X2_FROZEN);
        }
        if id == 0 || self.samples.iter().any(|s| s.id == id) {
            return Err(X2_DUP_SAMPLE);
        }
        self.samples.push(UsSample {
            id,
            nchars,
            tag: tag.to_string(),
        });
        Ok(())
    }

    /// 样本回读。
    pub fn sample_of(&self, id: u32) -> Option<&UsSample> {
        self.samples.iter().find(|s| s.id == id)
    }

    /// 全册复核。
    pub fn assert_all(&self) -> bool {
        let mut prev_end: u32 = 0;
        for f in &self.fields {
            if f.off % UsXrefBook::field_align(f.size) != 0
                || f.off < prev_end
                || f.end() > US_STRUCT_ANCHOR
            {
                return false;
            }
            prev_end = f.end();
        }
        true
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for f in &self.fields {
            h = h.wrapping_mul(211).wrapping_add(f.off);
            h = h.wrapping_mul(211).wrapping_add(f.size);
            for by in f.name.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        for s in &self.samples {
            h = h.wrapping_mul(211).wrapping_add(s.id);
            h = h.wrapping_mul(211).wrapping_add(s.nchars);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("fields={} samples={}", self.fields.len(), self.samples.len())
    }

    /// 诊断面：字段渲染行。
    pub fn line_of(&self, name: &str) -> String {
        match self.fields.iter().find(|f| f.name == name) {
            Some(f) => f.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = UsXrefBook::new();
    let _ = b.archive(UsField {
        name: "Length".to_string(), off: 0, size: 2,
    });
    // 1) 未对齐必拒。
    got[0] = b.archive(UsField {
        name: "A".to_string(), off: 3, size: 2,
    }).err() == Some(X2_UNALIGNED);
    // 2) 重叠必拒。
    got[1] = b.archive(UsField {
        name: "B".to_string(), off: 1, size: 2,
    }).err() == Some(X2_OVERLAP);
    // 3) 超锚必拒。
    got[2] = b.archive(UsField {
        name: "C".to_string(), off: 8, size: 16,
    }).err() == Some(X2_EXCEED);
    // 4) 重样本必拒（含零号）。
    let _ = b.add_sample(1, 3, "a");
    got[3] = b.add_sample(1, 4, "b").err() == Some(X2_DUP_SAMPLE)
        && b.add_sample(0, 4, "z").err() == Some(X2_DUP_SAMPLE);
    // 5) 印账后归档必抓。
    b.seal();
    got[4] = b.archive(UsField {
        name: "D".to_string(), off: 8, size: 8,
    }).err() == Some(X2_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12118_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12118");

    // J1 三字段闭集归档（x64：Length@0/2、MaximumLength@2/2、Buffer@8/8）。
    let j1 = {
        let mut b = UsXrefBook::new();
        let _ = b.archive(UsField { name: "Length".to_string(), off: 0, size: 2 });
        let _ = b.archive(UsField { name: "MaximumLength".to_string(), off: 2, size: 2 });
        let _ = b.archive(UsField { name: "Buffer".to_string(), off: 8, size: 8 });
        b.assert_all() && b.fields.len() == 3
    };
    s.add("J1-三字段闭集", j1, "三档全归档");

    // J2 未对齐拒（8 字节档 off=4 违 8 对齐口径）。
    let j2 = {
        let mut b = UsXrefBook::new();
        b.archive(UsField { name: "A".to_string(), off: 4, size: 8 })
            .err() == Some(X2_UNALIGNED)
    };
    s.add("J2-未对齐拒", j2, "非口径对齐必拒");

    // J3 重叠拒。
    let j3 = {
        let mut b = UsXrefBook::new();
        let _ = b.archive(UsField { name: "A".to_string(), off: 0, size: 4 });
        b.archive(UsField { name: "B".to_string(), off: 2, size: 2 })
            .err() == Some(X2_OVERLAP)
    };
    s.add("J3-重叠拒", j3, "区间相交必拒");

    // J4 超锚拒。
    let j4 = {
        let mut b = UsXrefBook::new();
        b.archive(UsField { name: "Big".to_string(), off: 8, size: 24 })
            .err() == Some(X2_EXCEED)
    };
    s.add("J4-超锚拒", j4, "尾出 16 必拒");

    // J5 贴锚放行。
    let j5 = {
        let mut b = UsXrefBook::new();
        let _ = b.archive(UsField { name: "A".to_string(), off: 0, size: 4 });
        b.archive(UsField { name: "B".to_string(), off: 8, size: 8 }).is_ok()
            && b.fields[1].end() == US_STRUCT_ANCHOR
    };
    s.add("J5-贴锚放行", j5, "恰达 16 不拒");

    // J6 样本登记与推导口径。
    let j6 = {
        let mut b = UsXrefBook::new();
        let _ = b.add_sample(1, 3, "abc");
        let sm = b.sample_of(1);
        match sm {
            Some(x) => x.derived() == (6, 8),
            None => false,
        }
    };
    s.add("J6-样本推导", j6, "len=n×2、max=len+2");

    // J7 重样本与零号拒。
    let j7 = {
        let mut b = UsXrefBook::new();
        let _ = b.add_sample(2, 1, "a");
        b.add_sample(2, 2, "b").err() == Some(X2_DUP_SAMPLE)
            && b.add_sample(0, 2, "z").err() == Some(X2_DUP_SAMPLE)
    };
    s.add("J7-样本唯一", j7, "零号重号皆拒");

    // J8 样本渲染。
    let j8 = {
        let mut b = UsXrefBook::new();
        let _ = b.add_sample(3, 5, "five");
        match b.sample_of(3) {
            Some(x) => x.render().contains("n5"),
            None => false,
        }
    };
    s.add("J8-样本渲染", j8, "样本行含字符数");

    // J9 字段渲染。
    let j9 = {
        let mut b = UsXrefBook::new();
        let _ = b.archive(UsField { name: "Buffer".to_string(), off: 8, size: 8 });
        b.line_of("Buffer").contains("8+8") && b.line_of("Nope") == "missing"
    };
    s.add("J9-字段渲染", j9, "字段行可渲染");

    // J10 印账冻结。
    let j10 = {
        let mut b = UsXrefBook::new();
        b.seal();
        b.archive(UsField { name: "P".to_string(), off: 0, size: 2 })
            .err() == Some(X2_FROZEN)
            && b.add_sample(1, 1, "p").err() == Some(X2_FROZEN)
    };
    s.add("J10-印账冻结", j10, "seal 后皆拒");

    // J11 指纹幂等。
    let j11 = {
        let mut b = UsXrefBook::new();
        let _ = b.archive(UsField { name: "A".to_string(), off: 0, size: 2 });
        b.fold() == b.fold()
    };
    s.add("J11-指纹幂等", j11, "同态同指纹");

    // J12 注入核 5/5。
    let got = inject_five();
    let j12 = got.iter().all(|x| *x);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = UsXrefBook::new();
    let _ = b.archive(UsField { name: "Length".to_string(), off: 0, size: 2 });
    let _ = b.archive(UsField { name: "MaximumLength".to_string(), off: 2, size: 2 });
    let _ = b.archive(UsField { name: "Buffer".to_string(), off: 8, size: 8 });
    let _ = b.add_sample(1, 8, "devices");
    let _ = b.add_sample(2, 0, "empty");

    s.add("摘要回读", b.summary().contains("fields=3 samples=2"), "三档两样本");
    s.add("全册复核", b.assert_all(), "逐档过断言");
    s.add("指纹非零", b.fold() != 0, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12118_archive_flow() {
        let mut b = UsXrefBook::new();
        assert!(b.archive(UsField { name: "A".to_string(), off: 0, size: 2 }).is_ok());
        assert_eq!(
            b.archive(UsField { name: "B".to_string(), off: 1, size: 2 }),
            Err(X2_OVERLAP)
        );
        assert!(b.archive(UsField { name: "C".to_string(), off: 8, size: 8 }).is_ok());
    }

    #[test]
    fn unx12118_checks_green() {
        let s = run_unx12118_checks();
        assert!(s.all_passed(), "UNX-F12118 判据未全绿");
    }

    #[test]
    fn sample_derivation() {
        let sm = UsSample { id: 1, nchars: 7, tag: "x".to_string() };
        assert_eq!(sm.derived(), (14, 16));
        assert!(sm.render().contains("n7"));
    }
}
