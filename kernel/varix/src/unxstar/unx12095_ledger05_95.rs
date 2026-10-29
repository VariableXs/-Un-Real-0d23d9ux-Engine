//! UNX-F12095 OBJECT_ATTRIBUTES 版本锚分档账（0x19e00 段）
//!
//! 卡面：OBJECT_ATTRIBUTES 版本锚——按 Length 分档（24 → x86 档、
//! 48 → x64 档），未知长度拒收；锚版本单调、同档单锚。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 分档闭集：仅 24/48 两档，其余长度（含零）拒收。
//! - J2 同档单锚：同档二次锚定必拒。
//! - J3 锚版本单调：锚版本回退必拒。
//! - J4 分档账回读：逐档（锚版本/样本数）可回读。
//! - J5 印账冻结：seal 后锚定必拒。
//! - J6 注入五况全抓。
//!
//! 结构：Grade 双档枚举 + AnchorBook 分档账 + 折叠指纹（211 进制卷积）+
//! 注入核（未知长度/同档双锚/版本回退/零长度/印后锚）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19e00 独占 0x20）。
pub const SEG_BASE: u32 = 0x19e00;
pub const VA_UNKNOWN_LEN: u32 = SEG_BASE + 0x01; // 未知长度
pub const VA_DUP_GRADE: u32 = SEG_BASE + 0x02; // 同档双锚
pub const VA_BACK_VERSION: u32 = SEG_BASE + 0x03; // 版本回退
pub const VA_ZERO_LEN: u32 = SEG_BASE + 0x04; // 零长度
pub const VA_FROZEN: u32 = SEG_BASE + 0x05; // 印账后锚定

/// x86/x64 档长。
pub const LEN_X86: u32 = 24;
pub const LEN_X64: u32 = 48;

/// 分档枚举。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    X86 = 1,
    X64 = 2,
}

impl Grade {
    pub fn from_len(len: u32) -> Result<Grade, u32> {
        match len {
            0 => Err(VA_ZERO_LEN),
            LEN_X86 => Ok(Grade::X86),
            LEN_X64 => Ok(Grade::X64),
            _ => Err(VA_UNKNOWN_LEN),
        }
    }
}

/// 档锚记录。
#[derive(Clone)]
pub struct GradeAnchor {
    pub grade: Grade,
    pub version: u32,
    pub samples: u32,
    pub note: String,
}

impl GradeAnchor {
    pub fn render(&self) -> String {
        format!("G{}|v{}|n{}", self.grade as u16, self.version, self.samples)
    }
}

/// 分档账。
pub struct AnchorBook {
    pub anchors: Vec<GradeAnchor>,
    pub top_version: u32,
    pub sealed: bool,
}

impl AnchorBook {
    pub fn new() -> AnchorBook {
        AnchorBook { anchors: Vec::new(), top_version: 1, sealed: false }
    }

    /// 锚定：分档 → 同档单锚 → 版本单调。
    pub fn anchor(&mut self, len: u32, version: u32, note: &str) -> Result<Grade, u32> {
        if self.sealed {
            return Err(VA_FROZEN);
        }
        let g = Grade::from_len(len)?;
        if self.anchors.iter().any(|a| a.grade == g) {
            return Err(VA_DUP_GRADE);
        }
        if version <= self.top_version {
            return Err(VA_BACK_VERSION);
        }
        self.top_version = version;
        self.anchors.push(GradeAnchor {
            grade: g,
            version,
            samples: 0,
            note: note.to_string(),
        });
        Ok(g)
    }

    /// 档内样本累加。
    pub fn bump_sample(&mut self, g: Grade) -> Result<(), u32> {
        if self.sealed {
            return Err(VA_FROZEN);
        }
        match self.anchors.iter_mut().find(|a| a.grade == g) {
            Some(a) => {
                a.samples += 1;
                Ok(())
            }
            None => Err(VA_DUP_GRADE), // 无锚档不可累样本
        }
    }

    /// 档样本数查询。
    pub fn samples_of(&self, g: Grade) -> Option<u32> {
        self.anchors.iter().find(|a| a.grade == g).map(|a| a.samples)
    }

    /// 全档齐备。
    pub fn complete(&self) -> bool {
        self.anchors.len() == 2
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for a in &self.anchors {
            h = h.wrapping_mul(211).wrapping_add(a.grade as u32);
            h = h.wrapping_mul(211).wrapping_add(a.version);
            h = h.wrapping_mul(211).wrapping_add(a.samples);
            for by in a.note.bytes() {
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
        format!("anchors={} top={}", self.anchors.len(), self.top_version)
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, g: Grade) -> String {
        match self.anchors.iter().find(|a| a.grade == g) {
            Some(a) => a.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = AnchorBook::new();
    // 1) 未知长度必拒。
    got[0] = b.anchor(32, 2, "x").err() == Some(VA_UNKNOWN_LEN);
    // 2) 零长度必拒。
    got[1] = b.anchor(0, 2, "z").err() == Some(VA_ZERO_LEN);
    // 3) 同档双锚必拒。
    let _ = b.anchor(LEN_X64, 2, "x64");
    got[2] = b.anchor(LEN_X64, 3, "dup").err() == Some(VA_DUP_GRADE);
    // 4) 版本回退必拒。
    got[3] = b.anchor(LEN_X86, 1, "back").err() == Some(VA_BACK_VERSION);
    // 5) 印账后锚定必抓。
    b.seal();
    got[4] = b.anchor(LEN_X86, 9, "post").err() == Some(VA_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12095_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12095");

    // J1 分档闭集。
    let j1 = Grade::from_len(32).err() == Some(VA_UNKNOWN_LEN)
        && Grade::from_len(16).err() == Some(VA_UNKNOWN_LEN)
        && Grade::from_len(LEN_X86).is_ok()
        && Grade::from_len(LEN_X64).is_ok();
    s.add("J1-分档闭集", j1, "24/48 两档");

    // J2 零长度拒。
    let j2 = Grade::from_len(0).err() == Some(VA_ZERO_LEN);
    s.add("J2-零长度拒", j2, "零长必拒");

    // J3 同档单锚。
    let j3 = {
        let mut b = AnchorBook::new();
        let _ = b.anchor(LEN_X86, 2, "x86");
        b.anchor(LEN_X86, 3, "dup").err() == Some(VA_DUP_GRADE)
    };
    s.add("J3-同档单锚", j3, "双锚必拒");

    // J4 锚版本单调。
    let j4 = {
        let mut b = AnchorBook::new();
        let _ = b.anchor(LEN_X64, 5, "v5");
        b.anchor(LEN_X86, 5, "flat").err() == Some(VA_BACK_VERSION)
            && b.anchor(LEN_X86, 3, "back").err() == Some(VA_BACK_VERSION)
            && b.anchor(LEN_X86, 6, "ok").is_ok()
    };
    s.add("J4-版本单调", j4, "平版本回退皆拒");

    // J5 双档齐备。
    let j5 = {
        let mut b = AnchorBook::new();
        let _ = b.anchor(LEN_X86, 2, "a");
        let _ = b.anchor(LEN_X64, 3, "b");
        b.complete()
    };
    s.add("J5-双档齐备", j5, "两锚齐备");

    // J6 样本累加与回读。
    let j6 = {
        let mut b = AnchorBook::new();
        let _ = b.anchor(LEN_X64, 2, "x64");
        let _ = b.bump_sample(Grade::X64);
        let _ = b.bump_sample(Grade::X64);
        b.samples_of(Grade::X64) == Some(2)
            && b.samples_of(Grade::X86).is_none()
    };
    s.add("J6-样本回读", j6, "逐档计数");

    // J7 无锚档累样本拒。
    let j7 = {
        let mut b = AnchorBook::new();
        b.bump_sample(Grade::X86).err() == Some(VA_DUP_GRADE)
    };
    s.add("J7-无锚拒累", j7, "未锚档不可累样本");

    // J8 渲染行。
    let j8 = {
        let mut b = AnchorBook::new();
        let _ = b.anchor(LEN_X86, 4, "x86");
        b.line_of(Grade::X86).contains("v4") && b.line_of(Grade::X64) == "missing"
    };
    s.add("J8-渲染行", j8, "锚行含版本");

    // J9 印账冻结。
    let j9 = {
        let mut b = AnchorBook::new();
        b.seal();
        b.anchor(LEN_X64, 9, "post").err() == Some(VA_FROZEN)
            && b.bump_sample(Grade::X86).err() == Some(VA_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后皆拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = AnchorBook::new();
        let _ = b.anchor(LEN_X64, 2, "a");
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = AnchorBook::new();
    let _ = b.anchor(LEN_X86, 2, "x86-anchor");
    let _ = b.anchor(LEN_X64, 3, "x64-anchor");
    let _ = b.bump_sample(Grade::X64);
    let _ = b.bump_sample(Grade::X64);
    let _ = b.bump_sample(Grade::X64);

    s.add("摘要回读", b.summary().contains("top=3"), "顶版本 3");
    s.add("样本分账", b.samples_of(Grade::X64) == Some(3)
        && b.samples_of(Grade::X86) == Some(0), "3/0 分账");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12095_grade_flow() {
        assert_eq!(Grade::from_len(48), Ok(Grade::X64));
        assert_eq!(Grade::from_len(24), Ok(Grade::X86));
        assert_eq!(Grade::from_len(40), Err(VA_UNKNOWN_LEN));
        let mut b = AnchorBook::new();
        assert!(b.anchor(48, 2, "a").is_ok());
        assert_eq!(b.anchor(48, 3, "b"), Err(VA_DUP_GRADE));
    }

    #[test]
    fn unx12095_checks_green() {
        let s = run_unx12095_checks();
        assert!(s.all_passed(), "UNX-F12095 判据未全绿");
    }

    #[test]
    fn anchor_render() {
        let a = GradeAnchor {
            grade: Grade::X64, version: 7, samples: 2, note: "n".to_string(),
        };
        assert!(a.render().contains("v7"));
    }
}
