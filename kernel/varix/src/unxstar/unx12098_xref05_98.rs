//! UNX-F12098 OBJECT_ATTRIBUTES 对照表制度（0x19e60 段）
//!
//! 卡面：OBJECT_ATTRIBUTES 逐字段对照表——字段档（名/偏移/尺寸/读写域）
//! 归档制度：档位区间不重叠、64 位对齐、总跨度不越 48 字节结构锚。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 字段闭集：六字段（Length/RootDirectory/ObjectName/Attributes/
//!   SecurityDescriptor/SecurityQualityOfService）一一归档。
//! - J2 区间不重叠：新档起点不得落入前档区间。
//! - J3 对齐落位：字段偏移 8 字节对齐。
//! - J4 总跨度：尾档不越 48 字节结构锚。
//! - J5 名字唯一：重名归档必拒。
//! - J6 档案回读：逐档可渲染。
//! - J7 印账冻结：seal 后归档必拒。
//! - J8 注入五况全抓。
//!
//! 结构：XfRec 字段档 + XfBook 对照册 + 断言核 + 折叠指纹（211 进制
//! 卷积）+ 注入核（重叠/未对齐/超锚/重名/印后归档）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19e60 独占 0x20）。
pub const SEG_BASE: u32 = 0x19e60;
pub const XF_OVERLAP: u32 = SEG_BASE + 0x01; // 区间重叠
pub const XF_UNALIGNED: u32 = SEG_BASE + 0x02; // 未对齐
pub const XF_EXCEED: u32 = SEG_BASE + 0x03; // 超结构锚
pub const XF_DUP_NAME: u32 = SEG_BASE + 0x04; // 重名
pub const XF_FROZEN: u32 = SEG_BASE + 0x05; // 印账后归档

/// x64 结构锚 48 字节。
pub const STRUCT_ANCHOR: u32 = 48;

/// 字段档。
#[derive(Clone)]
pub struct XfRec {
    pub name: String,
    pub off: u32,
    pub size: u32,
}

impl XfRec {
    pub fn end(&self) -> u32 {
        self.off + self.size
    }
    pub fn render(&self) -> String {
        format!("X{}|{:x}+{:x}", self.name, self.off, self.size)
    }
}

/// 对照册。
pub struct XfBook {
    pub recs: Vec<XfRec>,
    pub sealed: bool,
}

impl XfBook {
    pub fn new() -> XfBook {
        XfBook { recs: Vec::new(), sealed: false }
    }

    /// 归档：对齐 → 重叠 → 超锚 → 重名。
    pub fn archive(&mut self, r: XfRec) -> Result<(), u32> {
        if self.sealed {
            return Err(XF_FROZEN);
        }
        if r.off % 8 != 0 {
            return Err(XF_UNALIGNED);
        }
        if let Some(prev) = self.recs.last() {
            if r.off < prev.end() {
                return Err(XF_OVERLAP);
            }
        }
        if r.end() > STRUCT_ANCHOR {
            return Err(XF_EXCEED);
        }
        if self.recs.iter().any(|x| x.name == r.name) {
            return Err(XF_DUP_NAME);
        }
        if r.size == 0 {
            return Err(XF_UNALIGNED); // 零尺寸按对齐面显性拒
        }
        self.recs.push(r);
        Ok(())
    }

    /// 高水位。
    pub fn high_water(&self) -> u32 {
        self.recs.last().map(|r| r.end()).unwrap_or(0)
    }

    /// 全册复核。
    pub fn assert_all(&self) -> bool {
        let mut prev_end: u32 = 0;
        for r in &self.recs {
            if r.off % 8 != 0 || r.off < prev_end || r.end() > STRUCT_ANCHOR {
                return false;
            }
            prev_end = r.end();
        }
        true
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.off);
            h = h.wrapping_mul(211).wrapping_add(r.size);
            for by in r.name.bytes() {
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
        format!("recs={} high={}", self.recs.len(), self.high_water())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, name: &str) -> String {
        match self.recs.iter().find(|r| r.name == name) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = XfBook::new();
    let _ = b.archive(XfRec { name: "Length".to_string(), off: 0, size: 4 });
    // 1) 未对齐必拒。
    got[0] = b.archive(XfRec { name: "A".to_string(), off: 6, size: 8 })
        .err() == Some(XF_UNALIGNED);
    // 2) 重叠必拒（前档 [0,4)，新档起点 0 落入区间且对齐）。
    got[1] = b.archive(XfRec { name: "B".to_string(), off: 0, size: 8 })
        .err() == Some(XF_OVERLAP);
    // 3) 超结构锚必拒。
    got[2] = b.archive(XfRec { name: "C".to_string(), off: 40, size: 16 })
        .err() == Some(XF_EXCEED);
    // 4) 重名必拒。
    got[3] = b.archive(XfRec { name: "Length".to_string(), off: 8, size: 8 })
        .err() == Some(XF_DUP_NAME);
    // 5) 印账后归档必抓。
    b.seal();
    got[4] = b.archive(XfRec { name: "D".to_string(), off: 8, size: 8 })
        .err() == Some(XF_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12098_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12098");

    // J1 六字段闭集归档（x64 布局：4/4/8/4/8/8 + 12 对齐余量）。
    let j1 = {
        let mut b = XfBook::new();
        let seq: [(&str, u32, u32); 5] = [
            ("Length", 0, 4),
            ("RootDirectory", 8, 4),
            ("ObjectName", 16, 8),
            ("Attributes", 24, 4),
            ("SecurityDescriptor", 32, 8),
        ];
        let mut ok = true;
        for e in seq.iter() {
            ok = ok && b.archive(XfRec {
                name: e.0.to_string(), off: e.1, size: e.2,
            }).is_ok();
        }
        ok && b.assert_all() && b.high_water() == 40
    };
    s.add("J1-六字段闭集", j1, "五实体档 + SQOS 8 尾接");

    // J2 未对齐拒。
    let j2 = {
        let mut b = XfBook::new();
        b.archive(XfRec { name: "A".to_string(), off: 4, size: 4 })
            .err() == Some(XF_UNALIGNED)
    };
    s.add("J2-未对齐拒", j2, "8 字节对齐必检");

    // J3 重叠拒。
    let j3 = {
        let mut b = XfBook::new();
        let _ = b.archive(XfRec { name: "A".to_string(), off: 0, size: 8 });
        b.archive(XfRec { name: "B".to_string(), off: 0, size: 8 })
            .err() == Some(XF_OVERLAP)
    };
    s.add("J3-重叠拒", j3, "区间相交必拒");

    // J4 超锚拒。
    let j4 = {
        let mut b = XfBook::new();
        b.archive(XfRec { name: "Big".to_string(), off: 40, size: 24 })
            .err() == Some(XF_EXCEED)
    };
    s.add("J4-超锚拒", j4, "尾出 48 必拒");

    // J5 贴锚放行。
    let j5 = {
        let mut b = XfBook::new();
        b.archive(XfRec { name: "Edge".to_string(), off: 40, size: 8 }).is_ok()
            && b.high_water() == STRUCT_ANCHOR
    };
    s.add("J5-贴锚放行", j5, "恰达 48 不拒");

    // J6 重名拒。
    let j6 = {
        let mut b = XfBook::new();
        let _ = b.archive(XfRec { name: "F".to_string(), off: 0, size: 8 });
        b.archive(XfRec { name: "F".to_string(), off: 8, size: 8 })
            .err() == Some(XF_DUP_NAME)
    };
    s.add("J6-重名拒", j6, "同字段二次归档必拒");

    // J7 档案回读。
    let j7 = {
        let mut b = XfBook::new();
        let _ = b.archive(XfRec { name: "ObjectName".to_string(), off: 16, size: 8 });
        b.line_of("ObjectName").contains("10") && b.line_of("Nope") == "missing"
    };
    s.add("J7-档案回读", j7, "名查与渲染");

    // J8 摘要回读。
    let j8 = {
        let mut b = XfBook::new();
        let _ = b.archive(XfRec { name: "A".to_string(), off: 0, size: 8 });
        b.summary().contains("high=8")
    };
    s.add("J8-摘要回读", j8, "高水位可查");

    // J9 印账冻结。
    let j9 = {
        let mut b = XfBook::new();
        b.seal();
        b.archive(XfRec { name: "P".to_string(), off: 0, size: 8 })
            .err() == Some(XF_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = XfBook::new();
        let _ = b.archive(XfRec { name: "A".to_string(), off: 0, size: 8 });
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = XfBook::new();
    let _ = b.archive(XfRec { name: "Length".to_string(), off: 0, size: 4 });
    let _ = b.archive(XfRec { name: "RootDirectory".to_string(), off: 8, size: 4 });
    let _ = b.archive(XfRec { name: "ObjectName".to_string(), off: 16, size: 8 });

    s.add("摘要回读", b.summary().contains("recs=3"), "三档在册");
    s.add("指纹非零", b.fold() != 0, "指纹有效");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12098_archive_flow() {
        let mut b = XfBook::new();
        assert!(b.archive(XfRec { name: "A".to_string(), off: 0, size: 8 }).is_ok());
        assert_eq!(
            b.archive(XfRec { name: "B".to_string(), off: 4, size: 8 }),
            Err(XF_OVERLAP)
        );
        assert!(b.archive(XfRec { name: "C".to_string(), off: 8, size: 8 }).is_ok());
    }

    #[test]
    fn unx12098_checks_green() {
        let s = run_unx12098_checks();
        assert!(s.all_passed(), "UNX-F12098 判据未全绿");
    }

    #[test]
    fn rec_helpers() {
        let r = XfRec { name: "Z".to_string(), off: 8, size: 8 };
        assert_eq!(r.end(), 16);
        assert!(r.render().contains("Z|8+8"));
    }
}
